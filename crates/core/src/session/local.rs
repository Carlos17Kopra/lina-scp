//! Local terminal session backed by a pseudo terminal (portable-pty).

use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{OutputSink, Session, SessionEvent};
use crate::error::{CoreError, Result};

const FALLBACK_SHELL: &str = "/bin/bash";

/// Picks the login shell: `$SHELL` if set and non-empty, else `/bin/bash`.
pub(crate) fn default_shell(shell_env: Option<String>) -> String {
    match shell_env {
        Some(s) if !s.trim().is_empty() => s,
        _ => FALLBACK_SHELL.to_string(),
    }
}

fn pty_error(context: &str, err: impl std::fmt::Display) -> CoreError {
    CoreError::Io { message: format!("{context}: {err}") }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// A shell running in a local pseudo terminal.
pub struct LocalPtySession {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl LocalPtySession {
    /// Spawns `$SHELL` (fallback `/bin/bash`) in the home directory with
    /// `TERM=xterm-256color`.
    pub fn spawn(cols: u16, rows: u16, sink: OutputSink) -> Result<Self> {
        let shell = default_shell(std::env::var("SHELL").ok());
        let mut cmd = CommandBuilder::new(shell);
        if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }
        Self::spawn_with_command(cmd, cols, rows, sink)
    }

    /// Spawns an arbitrary command in a new PTY. Used by `spawn` and tests.
    pub(crate) fn spawn_with_command(mut cmd: CommandBuilder, cols: u16, rows: u16, sink: OutputSink) -> Result<Self> {
        cmd.env("TERM", "xterm-256color");
        let pair = native_pty_system().openpty(pty_size(cols, rows)).map_err(|e| pty_error("cannot open pty", e))?;
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| pty_error("cannot spawn shell", e))?;
        // Close our copy of the slave so EOF is seen once the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| pty_error("cannot read pty", e))?;
        let writer = pair.master.take_writer().map_err(|e| pty_error("cannot write pty", e))?;
        let killer = child.clone_killer();

        std::thread::Builder::new()
            .name("linascp-pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => sink(SessionEvent::Output(buf[..n].to_vec())),
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        // EIO on Linux once the slave side is closed.
                        Err(_) => break,
                    }
                }
                let code = child.wait().ok().map(|status| status.exit_code());
                sink(SessionEvent::Exit { code });
            })
            .map_err(|e| pty_error("cannot start reader thread", e))?;

        Ok(Self { master: Mutex::new(pair.master), writer: Mutex::new(writer), killer: Mutex::new(killer) })
    }
}

#[async_trait::async_trait]
impl Session for LocalPtySession {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner());
        master.resize(pty_size(cols, rows)).map_err(|e| pty_error("cannot resize pty", e))
    }

    async fn close(&self) -> Result<()> {
        let mut killer = self.killer.lock().unwrap_or_else(|e| e.into_inner());
        // Fails with ESRCH if the child already exited; that is fine.
        let _ = killer.kill();
        Ok(())
    }
}

impl Drop for LocalPtySession {
    fn drop(&mut self) {
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;

    fn channel_sink() -> (OutputSink, mpsc::UnboundedReceiver<SessionEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: OutputSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        (sink, rx)
    }

    #[test]
    fn default_shell_uses_env_or_bash() {
        assert_eq!(default_shell(Some("/usr/bin/zsh".into())), "/usr/bin/zsh");
        assert_eq!(default_shell(Some("  ".into())), "/bin/bash");
        assert_eq!(default_shell(None), "/bin/bash");
    }

    #[tokio::test]
    async fn local_pty_echo_resize_close() {
        let (sink, mut rx) = channel_sink();
        let session =
            LocalPtySession::spawn_with_command(CommandBuilder::new("/bin/sh"), 80, 24, sink).expect("spawn /bin/sh");

        // The quotes keep the typed command (echoed back by the tty) from
        // matching; only the command output contains "hello".
        session.write(b"echo hel\"\"lo\n").await.unwrap();
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Output(bytes) = event {
                    output.extend_from_slice(&bytes);
                    if String::from_utf8_lossy(&output).contains("hello") {
                        break;
                    }
                }
            }
        })
        .await
        .expect("output contains hello");

        session.resize(120, 40).await.expect("resize");

        session.close().await.unwrap();
        let exited = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Exit { .. } = event {
                    return true;
                }
            }
            false
        })
        .await
        .expect("exit within timeout");
        assert!(exited);
    }

    #[tokio::test]
    async fn local_pty_reports_exit_code() {
        let (sink, mut rx) = channel_sink();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "exit 3"]);
        let _session = LocalPtySession::spawn_with_command(cmd, 80, 24, sink).unwrap();
        let code = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Exit { code } = event {
                    return code;
                }
            }
            None
        })
        .await
        .expect("exit within timeout");
        assert_eq!(code, Some(3));
    }
}
