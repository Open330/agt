use super::LlmCli;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;

/// Invoke an LLM CLI with a prompt and return the output.
/// Uses stdin to pass prompts to avoid OS ARG_MAX limits.
/// Streams stdout in real-time so users can see progress.
pub fn invoke(cli: LlmCli, prompt: &str) -> Result<String> {
    let mut child = match cli {
        LlmCli::Codex => {
            let sandbox = codex_sandbox_mode();
            Command::new("codex")
                .args(["exec", "--sandbox", &sandbox, "--skip-git-repo-check", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Failed to spawn codex")?
        }

        LlmCli::Claude => Command::new("claude")
            .args(["-p", "-", "--output-format", "text", "--dangerously-skip-permissions"])
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

    let status = child.wait().context(format!("Failed to wait for {}", cli))?;
    let _ = writer.join();

    if !status.success() {
        let stderr_output = child.stderr.take()
            .and_then(|mut s| {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut s, &mut buf).ok()?;
                Some(buf)
            })
            .unwrap_or_default();
        bail!("{} failed: {}", cli, stderr_output);
    }

    Ok(output)
}

/// Resolve the codex `--sandbox` mode.
///
/// An explicit `AGT_CODEX_SANDBOX` (read-only | workspace-write |
/// danger-full-access) always wins. Otherwise default to `workspace-write`,
/// but auto-fall back to `danger-full-access` on Linux hosts where the kernel
/// blocks unprivileged user namespaces: codex's sandbox uses bubblewrap, which
/// needs them, so on hardened hosts (e.g. Ubuntu 24.04's AppArmor userns
/// restriction) the sandboxed run would otherwise fail to start.
fn codex_sandbox_mode() -> String {
    if let Ok(mode) = std::env::var("AGT_CODEX_SANDBOX") {
        return mode;
    }

    #[cfg(target_os = "linux")]
    if !userns_available() {
        crate::ui::warn(
            "Unprivileged user namespaces are blocked; running codex without its OS sandbox \
             (danger-full-access). Set AGT_CODEX_SANDBOX to override.",
        );
        return "danger-full-access".to_string();
    }

    "workspace-write".to_string()
}

/// Probe whether this process can create an unprivileged user namespace,
/// which is exactly what codex's bubblewrap sandbox requires. Tested in a
/// throwaway child so the current process is left untouched.
#[cfg(target_os = "linux")]
fn userns_available() -> bool {
    unsafe {
        match libc::fork() {
            // Child: try to enter a new user namespace and report via exit code.
            0 => {
                let rc = libc::unshare(libc::CLONE_NEWUSER);
                libc::_exit(if rc == 0 { 0 } else { 1 });
            }
            // Fork failed unexpectedly — don't disable the sandbox over a fluke.
            -1 => true,
            pid => {
                let mut status: libc::c_int = 0;
                libc::waitpid(pid, &mut status, 0);
                libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0
            }
        }
    }
}
