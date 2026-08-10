use super::LlmCli;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;

fn codex_args(sandbox: Option<&str>) -> Vec<String> {
    let mut args = vec!["exec".into()];
    if let Some(mode) = sandbox {
        args.extend(["--sandbox".into(), mode.into()]);
    }
    args.extend(["--skip-git-repo-check".into(), "-".into()]);
    args
}

fn claude_args() -> [&'static str; 4] {
    ["-p", "-", "--output-format", "text"]
}

/// Invoke an LLM CLI with a prompt and return the output.
/// Uses stdin to pass prompts to avoid OS ARG_MAX limits.
/// Streams stdout in real-time so users can see progress.
pub fn invoke(cli: LlmCli, prompt: &str) -> Result<String> {
    let mut child = match cli {
        LlmCli::Codex => {
            let sandbox = std::env::var("AGT_CODEX_SANDBOX").ok();
            let args = codex_args(sandbox.as_deref());
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

    // Write prompt on a separate thread to avoid pipe deadlocks
    let mut stdin = child.stdin.take().context("Failed to open stdin")?;
    let prompt_owned = prompt.to_string();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(prompt_owned.as_bytes());
        // stdin is dropped here, sending EOF
    });

    // Drain stderr on its own thread. A chatty child (e.g. codex forwarding
    // verbose MCP-server logs) can write more than the ~64KB pipe buffer to
    // stderr; if we only read it after wait() — as we used to — the child blocks
    // on write(stderr), stops producing stdout, and the stdout loop below blocks
    // forever waiting for output that never comes. Same deadlock the stdin
    // writer thread above guards against, just on the other stream.
    let stderr = child.stderr.take().context("Failed to open stderr")?;
    let stderr_reader = thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        let _ = std::io::Read::read_to_string(&mut reader, &mut buf);
        buf
    });

    // Stream stdout line-by-line in real-time
    let stdout = child.stdout.take().context("Failed to open stdout")?;
    let reader = BufReader::new(stdout);
    let mut output = String::new();

    for line in reader.lines() {
        match line {
            Ok(line) => {
                println!("{}", line);
                output.push_str(&line);
                output.push('\n');
            }
            Err(_) => break,
        }
    }

    let status = child
        .wait()
        .context(format!("Failed to wait for {}", cli))?;
    let _ = writer.join();
    let stderr_output = stderr_reader.join().unwrap_or_default();

    if !status.success() {
        bail!("{} failed: {}", cli, stderr_output);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{claude_args, codex_args};

    #[test]
    fn codex_uses_safe_defaults() {
        let args = codex_args(None);

        assert_eq!(args, ["exec", "--skip-git-repo-check", "-"]);
        assert!(!args.iter().any(|arg| arg.contains("dangerously")));
    }

    #[test]
    fn codex_preserves_explicit_sandbox_modes() {
        for mode in ["read-only", "workspace-write", "danger-full-access"] {
            assert_eq!(
                codex_args(Some(mode)),
                ["exec", "--sandbox", mode, "--skip-git-repo-check", "-"]
            );
        }
    }

    #[test]
    fn claude_uses_safe_defaults() {
        let args = claude_args();

        assert_eq!(args, ["-p", "-", "--output-format", "text"]);
        assert!(!args.iter().any(|arg| arg.contains("dangerously")));
    }
}
