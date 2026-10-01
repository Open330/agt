use super::LlmCli;
use anyhow::{anyhow, bail, Context, Result};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;

fn codex_args(sandbox: Option<&str>) -> Result<Vec<String>> {
    let mut args = vec!["exec".into()];
    if let Some(mode) = sandbox {
        if !matches!(mode, "read-only" | "workspace-write" | "danger-full-access") {
            bail!(
                "Invalid AGT_CODEX_SANDBOX: expected read-only, workspace-write, or danger-full-access"
            );
        }
        args.extend(["--sandbox".into(), mode.into()]);
    }
    args.extend(["--skip-git-repo-check".into(), "-".into()]);
    Ok(args)
}

fn claude_args() -> [&'static str; 4] {
    ["-p", "-", "--output-format", "text"]
}

fn write_prompt(mut writer: impl Write, prompt: &str) -> io::Result<()> {
    writer.write_all(prompt.as_bytes())
}

fn stream_stdout(reader: impl BufRead) -> io::Result<String> {
    let mut output = String::new();

    for line in reader.lines() {
        let line = line?;
        println!("{}", line);
        output.push_str(&line);
        output.push('\n');
    }

    Ok(output)
}

fn read_stderr(mut reader: impl Read) -> io::Result<String> {
    let mut output = String::new();
    reader.read_to_string(&mut output)?;
    Ok(output)
}

fn join_io_worker<T>(worker: thread::JoinHandle<io::Result<T>>, operation: &str) -> Result<T> {
    worker
        .join()
        .map_err(|_| anyhow!("{operation} worker panicked"))?
        .with_context(|| format!("Failed to {operation}"))
}

fn invoke_child(child: Child, cli: &str, prompt: &str) -> Result<String> {
    invoke_child_with_wait(child, cli, prompt, Child::wait)
}

fn invoke_child_with_wait(
    mut child: Child,
    cli: &str,
    prompt: &str,
    wait: impl FnOnce(&mut Child) -> io::Result<ExitStatus>,
) -> Result<String> {
    // Write prompt on a separate thread to avoid pipe deadlocks
    let mut stdin = child.stdin.take().context("Failed to open stdin")?;
    let prompt_owned = prompt.to_string();
    let writer = thread::spawn(move || {
        write_prompt(&mut stdin, &prompt_owned)
        // stdin is dropped when this closure returns, sending EOF
    });

    // Drain stderr on its own thread. A chatty child (e.g. codex forwarding
    // verbose MCP-server logs) can write more than the ~64KB pipe buffer to
    // stderr; if we only read it after wait() — as we used to — the child blocks
    // on write(stderr), stops producing stdout, and the stdout loop below blocks
    // forever waiting for output that never comes. Same deadlock the stdin
    // writer thread above guards against, just on the other stream.
    let stderr = child.stderr.take().context("Failed to open stderr")?;
    let stderr_reader = thread::spawn(move || read_stderr(BufReader::new(stderr)));

    // Stream stdout line-by-line in real-time
    let stdout = child.stdout.take().context("Failed to open stdout")?;
    let output = stream_stdout(BufReader::new(stdout));

    let status = wait(&mut child).with_context(|| format!("Failed to wait for {cli}"));
    let writer = join_io_worker(writer, "write prompt");
    let stderr_output = join_io_worker(stderr_reader, "read stderr");

    // Resolve every transport component after all workers and the child have
    // been reaped. Output is accepted only if every component succeeded.
    writer?;
    let output = output.with_context(|| format!("Failed to read {cli} stdout"))?;
    let stderr_output = stderr_output?;
    let status = status?;

    if !status.success() {
        bail!("{cli} failed: {stderr_output}");
    }

    Ok(output)
}

/// Invoke an LLM CLI with a prompt and return the output.
/// Uses stdin to pass prompts to avoid OS ARG_MAX limits.
/// Streams stdout in real-time so users can see progress.
pub fn invoke(cli: LlmCli, prompt: &str) -> Result<String> {
    let child = match cli {
        LlmCli::Codex => {
            let sandbox = match std::env::var("AGT_CODEX_SANDBOX") {
                Ok(mode) => Some(mode),
                Err(std::env::VarError::NotPresent) => None,
                Err(std::env::VarError::NotUnicode(_)) => {
                    bail!("Invalid AGT_CODEX_SANDBOX: expected valid Unicode")
                }
            };
            let args = codex_args(sandbox.as_deref())?;
            Command::new("codex")
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Failed to spawn codex")?
        }

        LlmCli::Claude => Command::new("claude")
            .args(claude_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn claude")?,

        LlmCli::OpenCode => Command::new("opencode")
            .args(["run", "-q", "-f", "text", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn opencode")?,

        LlmCli::Gemini => Command::new("gemini")
            .args(["-p", "-", "-o", "text"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn gemini")?,

        LlmCli::Ollama => {
            let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string());
            Command::new("ollama")
                .args(["run", &model])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Failed to spawn ollama")?
        }
    };

    invoke_child(child, &cli.to_string(), prompt)
}

#[cfg(test)]
mod tests {
    use super::{
        claude_args, codex_args, invoke_child, invoke_child_with_wait, join_io_worker, read_stderr,
        stream_stdout, write_prompt,
    };
    use std::io::{self, BufReader, Cursor, Read, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    const FAKE_CLI_MODE: &str = "AGT_INVOKE_FAKE_CLI_MODE";

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken writer"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BrokenReader;

    impl Read for BrokenReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken reader"))
        }
    }

    fn fake_child(mode: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--quiet",
                "--ignored",
                "--exact",
                "llm::invoke::tests::fake_cli",
            ])
            .env(FAKE_CLI_MODE, mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[test]
    #[ignore]
    fn fake_cli() {
        let mode = std::env::var(FAKE_CLI_MODE).unwrap();

        match mode.as_str() {
            "success" => {
                let mut prompt = Vec::new();
                io::stdin().read_to_end(&mut prompt).unwrap();
                writeln!(io::stdout(), "prompt-bytes={}", prompt.len()).unwrap();
                std::process::exit(0);
            }
            "nonzero" => {
                io::stdout().write_all(b"partial output\n").unwrap();
                io::stderr().write_all(b"fake rejection").unwrap();
                std::process::exit(17);
            }
            "invalid-stdout" => {
                io::stdout().write_all(&[0xff, b'\n']).unwrap();
                std::process::exit(0);
            }
            "invalid-stderr" => {
                io::stdout().write_all(b"unacceptable output\n").unwrap();
                io::stderr().write_all(&[0xff]).unwrap();
                std::process::exit(0);
            }
            "close-stdin" => std::process::exit(0),
            "chatty" => {
                io::stderr().write_all(&vec![b'e'; 256 * 1024]).unwrap();
                let mut prompt = Vec::new();
                io::stdin().read_to_end(&mut prompt).unwrap();
                writeln!(io::stdout(), "prompt-bytes={}", prompt.len()).unwrap();
                std::process::exit(0);
            }
            _ => panic!("unknown fake CLI mode: {mode}"),
        }
    }

    #[test]
    fn codex_uses_safe_defaults() {
        let args = codex_args(None).unwrap();

        assert_eq!(args, ["exec", "--skip-git-repo-check", "-"]);
        assert!(!args.iter().any(|arg| arg.contains("dangerously")));
    }

    #[test]
    fn codex_preserves_explicit_sandbox_modes() {
        for mode in ["read-only", "workspace-write", "danger-full-access"] {
            assert_eq!(
                codex_args(Some(mode)).unwrap(),
                ["exec", "--sandbox", mode, "--skip-git-repo-check", "-"]
            );
        }
    }

    #[test]
    fn codex_rejects_invalid_sandbox_modes() {
        for mode in [
            "",
            " read-only",
            "read-only ",
            "--dangerously-bypass-approvals-and-sandbox",
            "unsupported",
        ] {
            assert!(
                codex_args(Some(mode)).is_err(),
                "sandbox mode {mode:?} should be rejected"
            );
        }
    }

    #[test]
    fn claude_uses_safe_defaults() {
        let args = claude_args();

        assert_eq!(args, ["-p", "-", "--output-format", "text"]);
        assert!(!args.iter().any(|arg| arg.contains("dangerously")));
    }

    #[test]
    fn prompt_write_errors_are_propagated() {
        let error = write_prompt(BrokenWriter, "prompt").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn stream_read_errors_are_propagated() {
        let stdout_error = stream_stdout(BufReader::new(BrokenReader)).unwrap_err();
        let stderr_error = read_stderr(BrokenReader).unwrap_err();

        assert_eq!(stdout_error.kind(), io::ErrorKind::Other);
        assert_eq!(stderr_error.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn stream_decode_errors_are_propagated() {
        let stdout_error = stream_stdout(BufReader::new(Cursor::new([0xff, b'\n']))).unwrap_err();
        let stderr_error = read_stderr(Cursor::new([0xff])).unwrap_err();

        assert_eq!(stdout_error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(stderr_error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn worker_panics_are_propagated() {
        let worker = std::thread::spawn(|| -> io::Result<()> { panic!("worker panic") });

        let error = join_io_worker(worker, "test operation").unwrap_err();

        assert!(error.to_string().contains("test operation worker panicked"));
    }

    #[test]
    fn invoke_accepts_output_only_after_success() {
        let output = invoke_child(fake_child("success"), "fake", "hello").unwrap();

        assert!(output.contains("prompt-bytes=5\n"));
    }

    #[test]
    fn invoke_rejects_nonzero_child_status() {
        let error = invoke_child(fake_child("nonzero"), "fake", "hello").unwrap_err();

        assert!(error.to_string().contains("fake failed: fake rejection"));
    }

    #[test]
    fn invoke_rejects_invalid_child_output() {
        let stdout_error = invoke_child(fake_child("invalid-stdout"), "fake", "").unwrap_err();
        let stderr_error = invoke_child(fake_child("invalid-stderr"), "fake", "").unwrap_err();

        assert!(stdout_error
            .to_string()
            .contains("Failed to read fake stdout"));
        assert!(stderr_error.to_string().contains("Failed to read stderr"));
    }

    #[test]
    fn invoke_propagates_prompt_close_failure() {
        let prompt = "p".repeat(16 * 1024 * 1024);
        let error = invoke_child(fake_child("close-stdin"), "fake", &prompt).unwrap_err();

        assert!(error.to_string().contains("Failed to write prompt"));
    }

    #[test]
    fn invoke_drains_chatty_stderr_while_writing_large_prompt() {
        let prompt = "p".repeat(1024 * 1024);
        let output = invoke_child(fake_child("chatty"), "fake", &prompt).unwrap();

        assert!(output.contains("prompt-bytes=1048576\n"));
    }

    #[test]
    fn invoke_propagates_wait_failure_after_reaping_child() {
        let reaped = Arc::new(AtomicBool::new(false));
        let reaped_in_wait = Arc::clone(&reaped);
        let error = invoke_child_with_wait(fake_child("success"), "fake", "hello", move |child| {
            child.wait()?;
            reaped_in_wait.store(true, Ordering::SeqCst);
            Err(io::Error::other("injected wait failure"))
        })
        .unwrap_err();

        assert!(reaped.load(Ordering::SeqCst));
        assert!(error.to_string().contains("Failed to wait for fake"));
    }
}
