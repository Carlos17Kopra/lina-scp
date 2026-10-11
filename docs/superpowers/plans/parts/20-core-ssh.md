### Task 8: Session trait, SessionManager and local PTY sessions

**Files:**
- Modify: `crates/core/Cargo.toml`
- Modify: `crates/core/src/lib.rs`
- Create: `crates/core/src/session/mod.rs`
- Create: `crates/core/src/session/local.rs`
- Test: unit tests inside both files (`cargo test -p linascp-core --lib session`)

**Interfaces:**
- Consumes: `crate::error::{CoreError, Result}` (Task 1–7; `CoreError` must derive `Clone` and `Debug`).
- Produces:
  ```rust
  pub type SessionId = uuid::Uuid;
  #[derive(Debug, Clone)] pub enum SessionEvent { Output(Vec<u8>), Exit { code: Option<u32> }, Error(CoreError) }
  pub type OutputSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;
  #[async_trait::async_trait] pub trait Session: Send + Sync {
      async fn write(&self, data: &[u8]) -> Result<()>;
      async fn resize(&self, cols: u16, rows: u16) -> Result<()>;
      async fn close(&self) -> Result<()>;
  }
  #[derive(Default)] pub struct SessionManager;   // fields private
  impl SessionManager {
      pub fn new() -> Self;
      pub fn insert(&self, id: SessionId, session: Arc<dyn Session>);
      pub fn get(&self, id: SessionId) -> Option<Arc<dyn Session>>;
      pub fn remove(&self, id: SessionId) -> Option<Arc<dyn Session>>;
  }
  pub struct LocalPtySession;  // fields private
  impl LocalPtySession {
      pub fn spawn(cols: u16, rows: u16, sink: OutputSink) -> Result<Self>;
      pub(crate) fn spawn_with_command(cmd: portable_pty::CommandBuilder, cols: u16, rows: u16, sink: OutputSink) -> Result<Self>;
  }
  ```

- [ ] **Step 1: Add dependencies and declare the module**

In `crates/core/Cargo.toml` add these lines to the existing `[dependencies]` table (keep alphabetical order; do not touch the entries from Task 1):

```toml
async-trait = "0.1"
portable-pty = "0.9"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync", "io-util", "fs"] }
tokio-util = "0.7"
```

(Already present from Task 1 and used here: `russh = "0.64"`, `secrecy = "0.10"`, `serde`, `serde_json`, `specta = "=2.0.0-rc.25"`, `thiserror = "2"`, `uuid` (v4), dev-dependency `tempfile = "3"`.)

In `crates/core/src/lib.rs` add:

```rust
pub mod session;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/session/mod.rs` with only the module declaration and tests:

```rust
//! Session abstraction shared by local PTYs and SSH shells.

pub mod local;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeSession {
        writes: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Session for FakeSession {
        async fn write(&self, _data: &[u8]) -> Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn resize(&self, _cols: u16, _rows: u16) -> Result<()> {
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn manager_insert_get_remove() {
        let manager = SessionManager::new();
        let id = SessionId::new_v4();
        let fake = Arc::new(FakeSession::default());
        manager.insert(id, fake.clone());

        let got = manager.get(id).expect("session registered");
        got.write(b"x").await.unwrap();
        assert_eq!(fake.writes.load(Ordering::SeqCst), 1);

        assert!(manager.remove(id).is_some());
        assert!(manager.get(id).is_none());
        assert!(manager.remove(id).is_none());
    }

    #[test]
    fn manager_get_unknown_is_none() {
        let manager = SessionManager::new();
        assert!(manager.get(SessionId::new_v4()).is_none());
    }
}
```

Create `crates/core/src/session/local.rs` with only the tests:

```rust
//! Local terminal session backed by a pseudo terminal (portable-pty).

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
            LocalPtySession::spawn_with_command(CommandBuilder::new("/bin/sh"), 80, 24, sink)
                .expect("spawn /bin/sh");

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p linascp-core --lib session`
Expected: compilation fails with `error[E0412]/E0433: cannot find type 'SessionManager'` / `'LocalPtySession'` / `'Session'` in this scope.

- [ ] **Step 4: Implement the session module**

In `crates/core/src/session/mod.rs` insert between `pub mod local;` and `#[cfg(test)]`:

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{CoreError, Result};

pub use local::LocalPtySession;

/// Identifier of a running session (assigned by the caller).
pub type SessionId = uuid::Uuid;

/// Events a session pushes to its consumer.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Raw terminal output bytes.
    Output(Vec<u8>),
    /// The session ended; `code` is the exit status if known.
    Exit { code: Option<u32> },
    /// A fatal error inside the session task.
    Error(CoreError),
}

/// Callback receiving session events. Must be cheap and non-blocking.
pub type OutputSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// Common interface of all interactive sessions.
#[async_trait::async_trait]
pub trait Session: Send + Sync {
    /// Send input bytes to the session.
    async fn write(&self, data: &[u8]) -> Result<()>;
    /// Inform the session about a new terminal size.
    async fn resize(&self, cols: u16, rows: u16) -> Result<()>;
    /// Terminate the session. An `Exit` event follows asynchronously.
    async fn close(&self) -> Result<()>;
}

/// Registry of running sessions, keyed by `SessionId`.
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<SessionId, Arc<dyn Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: SessionId, session: Arc<dyn Session>) {
        self.lock().insert(id, session);
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<dyn Session>> {
        self.lock().get(&id).cloned()
    }

    pub fn remove(&self, id: SessionId) -> Option<Arc<dyn Session>> {
        self.lock().remove(&id)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Arc<dyn Session>>> {
        // A poisoned map is still structurally valid; keep serving it.
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }
}
```

In `crates/core/src/session/local.rs` insert between the `//!` doc line and `#[cfg(test)]`:

```rust
use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

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
    CoreError::Io {
        message: format!("{context}: {err}"),
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
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
    pub(crate) fn spawn_with_command(
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        sink: OutputSink,
    ) -> Result<Self> {
        cmd.env("TERM", "xterm-256color");
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows))
            .map_err(|e| pty_error("cannot open pty", e))?;
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| pty_error("cannot spawn shell", e))?;
        // Close our copy of the slave so EOF is seen once the child exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| pty_error("cannot read pty", e))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| pty_error("cannot write pty", e))?;
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

        Ok(Self {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
        })
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
        master
            .resize(pty_size(cols, rows))
            .map_err(|e| pty_error("cannot resize pty", e))
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
```

(`writer.write_all(data)?` relies on `From<std::io::Error> for CoreError` from Task 1–7.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p linascp-core --lib session`
Expected: `test result: ok. 5 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add crates/core/Cargo.toml crates/core/src/lib.rs crates/core/src/session/mod.rs crates/core/src/session/local.rs
git commit -m "feat(core): add Session trait, SessionManager and local PTY sessions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Connect types, prompt broker, error classification, log sanitizer

**Files:**
- Modify: `crates/core/src/lib.rs`
- Create: `crates/core/src/connect/mod.rs`
- Create: `crates/core/src/connect/classify.rs`
- Create: `crates/core/src/connect/prompt.rs`
- Test: unit tests inside the three files (`cargo test -p linascp-core --lib connect`)

**Interfaces:**
- Consumes: `crate::error::{CoreError, NetworkKind}` (`NetworkKind: Debug + Clone + Copy + PartialEq`), `crate::known_hosts::{HostKeyInfo, KnownHosts}` (`HostKeyInfo: Debug + Clone + PartialEq + Eq + Serialize + Deserialize + specta::Type`), `crate::model::{Host, LoginProfile}` (`Debug + Clone`), `crate::secrets::SecretStore`, `crate::session::SessionId` (Task 8).
- Produces (`connect/mod.rs`):
  ```rust
  pub enum Step { JumpHost, Tcp, HostKey, Auth, Channel, Shell }        // serde camelCase, Copy
  pub enum ConnectEvent { Steps{steps}, StepStarted{step}, StepDone{step}, StepFailed{step, error},
                          Log{line}, Prompt{prompt_id /* json "promptId" */, prompt},
                          Connected{session_id /* json "sessionId" */}, Failed{error} }  // tag "kind"
  pub fn log_line(line: impl Into<String>) -> ConnectEvent;            // sanitized Log event
  pub struct ConnectTarget { pub host: Host, pub profile: Option<LoginProfile> }
  pub struct ConnectRequest { pub target: ConnectTarget, pub jump: Option<ConnectTarget>, pub cols: u16, pub rows: u16 }
  pub struct ConnectDeps { pub known_hosts: KnownHosts, pub secrets: Arc<dyn SecretStore>, pub agent_sock: Option<PathBuf> }
  pub fn plan_steps(req: &ConnectRequest) -> Vec<Step>;
  ```
  (`connect/classify.rs`, re-exported from `connect`):
  ```rust
  pub fn classify_io(err: &std::io::Error) -> NetworkKind;
  pub fn classify_resolve_error(err: &std::io::Error) -> NetworkKind;  // always Dns
  pub fn network_error(kind: NetworkKind, message: impl Into<String>) -> CoreError;
  pub fn map_russh_error(err: russh::Error) -> CoreError;
  pub fn sanitize_log_line(line: &str) -> String;
  ```
  (`connect/prompt.rs`, re-exported from `connect`):
  ```rust
  pub const PROMPT_TIMEOUT: Duration;                 // 5 min
  pub type EmitFn = Arc<dyn Fn(ConnectEvent) + Send + Sync>;
  pub struct KbdPrompt { pub text: String, pub echo: bool }
  pub enum Prompt { HostKeyUnknown{..}, HostKeyChanged{..}, KeyboardInteractive{..}, Password{..}, Passphrase{key_path /* json "keyPath" */} }
  pub enum HostKeyDecision { Reject, AcceptOnce, Trust, Replace }
  pub enum PromptAnswer { HostKey{decision}, Responses{values}, Secret{value, save}, Cancel }   // Debug masks secrets
  impl PromptBroker {
      pub fn new(emit: EmitFn) -> Self;
      pub fn with_timeout(emit: EmitFn, timeout: Duration) -> Self;
      pub async fn ask(&self, prompt: Prompt) -> PromptAnswer;
      pub async fn ask_cancellable(&self, emit: &EmitFn, prompt: Prompt, cancel: &CancellationToken) -> PromptAnswer;
      pub fn answer(&self, prompt_id: u64, answer: PromptAnswer) -> bool;
  }
  ```

- [ ] **Step 1: Write the failing classify tests**

In `crates/core/src/lib.rs` add:

```rust
pub mod connect;
```

Create `crates/core/src/connect/mod.rs`:

```rust
//! SSH connection pipeline: [JumpHost] -> Tcp -> HostKey -> Auth -> Channel -> Shell.

pub mod classify;
```

Create `crates/core/src/connect/classify.rs` with only the tests:

```rust
//! Error classification and log sanitizing for the connect pipeline.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_io_kinds() {
        let t = io::Error::new(io::ErrorKind::TimedOut, "t");
        let r = io::Error::new(io::ErrorKind::ConnectionRefused, "r");
        let o = io::Error::new(io::ErrorKind::PermissionDenied, "o");
        assert_eq!(classify_io(&t), NetworkKind::Timeout);
        assert_eq!(classify_io(&r), NetworkKind::Refused);
        assert_eq!(classify_io(&o), NetworkKind::Other);
    }

    #[test]
    fn resolve_errors_are_dns() {
        let e = io::Error::other("failed to lookup address information");
        assert_eq!(classify_resolve_error(&e), NetworkKind::Dns);
    }

    #[test]
    fn network_error_builds_variant() {
        match network_error(NetworkKind::Refused, "nope") {
            CoreError::Network { kind, message } => {
                assert_eq!(kind, NetworkKind::Refused);
                assert_eq!(message, "nope");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn russh_errors_are_mapped() {
        let io_err = russh::Error::IO(io::Error::new(io::ErrorKind::ConnectionRefused, "x"));
        assert!(matches!(
            map_russh_error(io_err),
            CoreError::Network {
                kind: NetworkKind::Refused,
                ..
            }
        ));
        assert!(matches!(
            map_russh_error(russh::Error::KeepaliveTimeout),
            CoreError::Network {
                kind: NetworkKind::Timeout,
                ..
            }
        ));
        assert!(matches!(
            map_russh_error(russh::Error::UnknownKey),
            CoreError::HostKeyRejected { .. }
        ));
        assert!(matches!(
            map_russh_error(russh::Error::Disconnect),
            CoreError::Network {
                kind: NetworkKind::Other,
                ..
            }
        ));
        assert!(matches!(
            map_russh_error(russh::Error::Inconsistent),
            CoreError::Internal { .. }
        ));
    }

    #[test]
    fn sanitize_masks_after_keywords() {
        assert_eq!(sanitize_log_line("password: hunter2"), "password: ***");
        assert_eq!(sanitize_log_line("Passphrase=abc def"), "Passphrase=***");
        assert_eq!(
            sanitize_log_line("debug1: sending PASSWORD secret"),
            "debug1: sending PASSWORD ***"
        );
        assert_eq!(
            sanitize_log_line("x passphrase y password z"),
            "x passphrase ***"
        );
    }

    #[test]
    fn sanitize_leaves_other_lines_alone() {
        assert_eq!(
            sanitize_log_line("debug1: Connecting to 10.0.0.1 port 22"),
            "debug1: Connecting to 10.0.0.1 port 22"
        );
        assert_eq!(
            sanitize_log_line("ends with password"),
            "ends with password"
        );
        assert_eq!(sanitize_log_line("password:  "), "password:  ");
        assert_eq!(
            sanitize_log_line("Ünïcode password: ü"),
            "Ünïcode password: ***"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core --lib connect::classify`
Expected: compilation fails with `cannot find function 'classify_io'` (and `sanitize_log_line`, `map_russh_error`, …) / `failed to resolve: use of undeclared crate or module 'io'`.

- [ ] **Step 3: Implement classify.rs**

In `crates/core/src/connect/classify.rs` insert between the `//!` line and `#[cfg(test)]`:

```rust
use std::io;

use crate::error::{CoreError, NetworkKind};

/// Classify a socket-level error.
pub fn classify_io(err: &io::Error) -> NetworkKind {
    match err.kind() {
        io::ErrorKind::TimedOut => NetworkKind::Timeout,
        io::ErrorKind::ConnectionRefused => NetworkKind::Refused,
        _ => NetworkKind::Other,
    }
}

/// Any failure of name resolution is reported as DNS error.
pub fn classify_resolve_error(_err: &io::Error) -> NetworkKind {
    NetworkKind::Dns
}

/// Shorthand for `CoreError::Network`.
pub fn network_error(kind: NetworkKind, message: impl Into<String>) -> CoreError {
    CoreError::Network {
        kind,
        message: message.into(),
    }
}

/// Map a russh transport error to a `CoreError`.
pub fn map_russh_error(err: russh::Error) -> CoreError {
    use russh::Error as E;
    let message = err.to_string();
    match err {
        E::IO(io_err) => network_error(classify_io(&io_err), io_err.to_string()),
        E::ConnectionTimeout | E::KeepaliveTimeout | E::InactivityTimeout | E::Elapsed(_) => {
            network_error(NetworkKind::Timeout, message)
        }
        E::UnknownKey => CoreError::HostKeyRejected { message },
        E::Disconnect | E::HUP | E::SendError | E::RecvError | E::ChannelOpenFailure(_) => {
            network_error(NetworkKind::Other, message)
        }
        _ => CoreError::Internal { message },
    }
}

const SECRET_KEYWORDS: [&str; 2] = ["password", "passphrase"];
const MASK: &str = "***";

/// Mask everything following a `password`/`passphrase` keyword
/// (case-insensitive), keeping the keyword and a `:`/`=`/space separator.
pub fn sanitize_log_line(line: &str) -> String {
    // ASCII lowercase keeps byte offsets identical to `line`.
    let lower = line.to_ascii_lowercase();
    let cut = SECRET_KEYWORDS
        .iter()
        .filter_map(|kw| lower.find(kw).map(|pos| pos + kw.len()))
        .min();
    let Some(mut end) = cut else {
        return line.to_string();
    };
    let bytes = line.as_bytes();
    while end < bytes.len() && matches!(bytes[end], b':' | b'=' | b' ' | b'\t') {
        end += 1;
    }
    if end >= line.len() {
        return line.to_string();
    }
    format!("{}{}", &line[..end], MASK)
}
```

- [ ] **Step 4: Run the classify tests to verify they pass**

Run: `cargo test -p linascp-core --lib connect::classify`
Expected: `test result: ok. 6 passed; 0 failed`

- [ ] **Step 5: Write the failing prompt broker and connect type tests**

Replace `crates/core/src/connect/mod.rs` with:

```rust
//! SSH connection pipeline: [JumpHost] -> Tcp -> HostKey -> Auth -> Channel -> Shell.

pub mod classify;
pub mod prompt;

#[cfg(test)]
mod tests {
    use super::*;

    fn request(with_jump: bool) -> ConnectRequest {
        let target = ConnectTarget {
            host: Host::new("web", "web.example.org"),
            profile: None,
        };
        ConnectRequest {
            jump: with_jump.then(|| ConnectTarget {
                host: Host::new("bastion", "bastion.example.org"),
                profile: None,
            }),
            target,
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn plan_steps_without_jump() {
        assert_eq!(
            plan_steps(&request(false)),
            vec![
                Step::Tcp,
                Step::HostKey,
                Step::Auth,
                Step::Channel,
                Step::Shell
            ]
        );
    }

    #[test]
    fn plan_steps_with_jump() {
        let steps = plan_steps(&request(true));
        assert_eq!(steps.first(), Some(&Step::JumpHost));
        assert_eq!(steps.len(), 6);
    }

    #[test]
    fn log_line_is_sanitized() {
        match log_line("password: hunter2") {
            ConnectEvent::Log { line } => assert_eq!(line, "password: ***"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let json = serde_json::to_value(ConnectEvent::StepStarted {
            step: Step::HostKey,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "stepStarted", "step": "hostKey"})
        );

        let json = serde_json::to_value(ConnectEvent::Prompt {
            prompt_id: 7,
            prompt: Prompt::Passphrase {
                key_path: "k".into(),
            },
        })
        .unwrap();
        assert_eq!(json["kind"], "prompt");
        assert_eq!(json["promptId"], 7);
        assert_eq!(json["prompt"]["kind"], "passphrase");

        let json = serde_json::to_value(ConnectEvent::Failed {
            error: CoreError::Cancelled,
        })
        .unwrap();
        assert_eq!(json["kind"], "failed");
        assert!(json["error"].is_object());
    }
}
```

Create `crates/core/src/connect/prompt.rs` with only the tests:

```rust
//! Interactive prompts during connection setup (host key, secrets, 2FA).

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_emit() -> EmitFn {
        Arc::new(|_| {})
    }

    /// Emitter that answers every prompt synchronously with `answer`.
    fn answering_emit(
        broker: Arc<PromptBroker>,
        answer: PromptAnswer,
        seen: Arc<Mutex<Vec<(u64, Prompt)>>>,
    ) -> EmitFn {
        Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, prompt } = event {
                seen.lock().unwrap().push((prompt_id, prompt));
                assert!(broker.answer(prompt_id, answer.clone()));
            }
        })
    }

    fn passphrase_prompt() -> Prompt {
        Prompt::Passphrase {
            key_path: "~/.ssh/id_ed25519".into(),
        }
    }

    #[tokio::test]
    async fn ask_returns_answer_and_emits_prompt() {
        let broker = Arc::new(PromptBroker::new(noop_emit()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let emit = answering_emit(
            broker.clone(),
            PromptAnswer::Secret {
                value: "pw".into(),
                save: true,
            },
            seen.clone(),
        );
        let answer = broker
            .ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new())
            .await;
        assert_eq!(
            answer,
            PromptAnswer::Secret {
                value: "pw".into(),
                save: true
            }
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, passphrase_prompt());
    }

    #[tokio::test]
    async fn prompt_ids_are_unique_across_brokers() {
        let ids = Arc::new(Mutex::new(Vec::new()));
        for _ in 0..2 {
            let broker = Arc::new(PromptBroker::new(noop_emit()));
            let ids2 = ids.clone();
            let b2 = broker.clone();
            let emit: EmitFn = Arc::new(move |event| {
                if let ConnectEvent::Prompt { prompt_id, .. } = event {
                    ids2.lock().unwrap().push(prompt_id);
                    b2.answer(prompt_id, PromptAnswer::Cancel);
                }
            });
            broker
                .ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new())
                .await;
            broker
                .ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new())
                .await;
        }
        let mut ids = ids.lock().unwrap().clone();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before);
        assert_eq!(before, 4);
    }

    #[test]
    fn answer_unknown_id_returns_false() {
        let broker = PromptBroker::new(noop_emit());
        assert!(!broker.answer(u64::MAX, PromptAnswer::Cancel));
    }

    #[tokio::test]
    async fn timeout_yields_cancel_and_forgets_id() {
        let last_id = Arc::new(Mutex::new(None));
        let last2 = last_id.clone();
        let emit: EmitFn = Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, .. } = event {
                *last2.lock().unwrap() = Some(prompt_id);
            }
        });
        let broker = PromptBroker::with_timeout(noop_emit(), Duration::from_millis(20));
        let answer = broker
            .ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new())
            .await;
        assert_eq!(answer, PromptAnswer::Cancel);
        let id = last_id.lock().unwrap().expect("prompt emitted");
        assert!(!broker.answer(id, PromptAnswer::Cancel));
    }

    #[tokio::test]
    async fn cancellation_yields_cancel() {
        let broker = PromptBroker::new(noop_emit());
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let emit: EmitFn = Arc::new(move |_| c2.cancel());
        let answer = broker
            .ask_cancellable(&emit, passphrase_prompt(), &cancel)
            .await;
        assert_eq!(answer, PromptAnswer::Cancel);
    }

    #[tokio::test]
    async fn ask_uses_own_emitter() {
        let slot: Arc<Mutex<Option<Arc<PromptBroker>>>> = Arc::new(Mutex::new(None));
        let slot2 = slot.clone();
        let emit: EmitFn = Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, .. } = event {
                let broker = slot2.lock().unwrap().clone().unwrap();
                broker.answer(
                    prompt_id,
                    PromptAnswer::HostKey {
                        decision: HostKeyDecision::Trust,
                    },
                );
            }
        });
        let broker = Arc::new(PromptBroker::new(emit));
        *slot.lock().unwrap() = Some(broker.clone());
        let answer = broker.ask(passphrase_prompt()).await;
        assert_eq!(
            answer,
            PromptAnswer::HostKey {
                decision: HostKeyDecision::Trust
            }
        );
    }

    #[test]
    fn debug_never_prints_secret() {
        let answer = PromptAnswer::Secret {
            value: "hunter2".into(),
            save: false,
        };
        assert!(!format!("{answer:?}").contains("hunter2"));
    }

    #[test]
    fn prompt_serializes_with_kind_tag() {
        let json = serde_json::to_value(passphrase_prompt()).unwrap();
        assert_eq!(json["kind"], "passphrase");
        assert_eq!(json["keyPath"], "~/.ssh/id_ed25519");
    }
}
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p linascp-core --lib connect`
Expected: compilation fails with `cannot find type 'PromptBroker'`, `cannot find type 'ConnectRequest'`, `cannot find function 'plan_steps'`, etc.

- [ ] **Step 7: Implement prompt.rs and the connect types**

In `crates/core/src/connect/prompt.rs` insert between the `//!` line and `#[cfg(test)]`:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::ConnectEvent;
use crate::known_hosts::HostKeyInfo;

/// How long the pipeline waits for an answer before treating it as `Cancel`.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Event callback used to publish connect events (and thus prompts).
pub type EmitFn = Arc<dyn Fn(ConnectEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KbdPrompt {
    pub text: String,
    pub echo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Prompt {
    HostKeyUnknown {
        host: String,
        port: u16,
        info: HostKeyInfo,
    },
    HostKeyChanged {
        host: String,
        port: u16,
        stored: HostKeyInfo,
        current: HostKeyInfo,
    },
    KeyboardInteractive {
        name: String,
        instructions: String,
        prompts: Vec<KbdPrompt>,
    },
    Password {
        user: String,
        host: String,
        retry: bool,
    },
    Passphrase {
        key_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyDecision {
    Reject,
    AcceptOnce,
    Trust,
    Replace,
}

/// Answer sent by the UI. `Debug` is implemented manually so secrets never
/// end up in logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PromptAnswer {
    HostKey { decision: HostKeyDecision },
    Responses { values: Vec<String> },
    Secret { value: String, save: bool },
    Cancel,
}

impl std::fmt::Debug for PromptAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptAnswer::HostKey { decision } => f
                .debug_struct("HostKey")
                .field("decision", decision)
                .finish(),
            PromptAnswer::Responses { values } => f
                .debug_struct("Responses")
                .field("count", &values.len())
                .finish(),
            PromptAnswer::Secret { save, .. } => f
                .debug_struct("Secret")
                .field("value", &"***")
                .field("save", save)
                .finish(),
            PromptAnswer::Cancel => f.write_str("Cancel"),
        }
    }
}

/// Process-wide unique prompt ids, so one broker can serve many connects.
static NEXT_PROMPT_ID: AtomicU64 = AtomicU64::new(1);

/// Routes prompts to the UI and answers back to the waiting pipeline.
pub struct PromptBroker {
    emit: EmitFn,
    pending: Mutex<HashMap<u64, oneshot::Sender<PromptAnswer>>>,
    timeout: Duration,
}

impl PromptBroker {
    /// Broker whose `ask` publishes prompts through `emit`.
    pub fn new(emit: EmitFn) -> Self {
        Self::with_timeout(emit, PROMPT_TIMEOUT)
    }

    /// Like `new` with a custom answer timeout (tests).
    pub fn with_timeout(emit: EmitFn, timeout: Duration) -> Self {
        Self {
            emit,
            pending: Mutex::new(HashMap::new()),
            timeout,
        }
    }

    /// Ask via the broker's own emitter; never cancelled externally.
    pub async fn ask(&self, prompt: Prompt) -> PromptAnswer {
        let emit = self.emit.clone();
        self.ask_cancellable(&emit, prompt, &CancellationToken::new())
            .await
    }

    /// Publish `prompt` through `emit` (the per-connect event channel) and
    /// wait for `answer`. Timeout or cancellation yields `PromptAnswer::Cancel`.
    pub async fn ask_cancellable(
        &self,
        emit: &EmitFn,
        prompt: Prompt,
        cancel: &CancellationToken,
    ) -> PromptAnswer {
        let prompt_id = NEXT_PROMPT_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        // Register before emitting so a synchronous answer is not lost.
        self.lock().insert(prompt_id, tx);
        emit(ConnectEvent::Prompt { prompt_id, prompt });

        let answer = tokio::select! {
            _ = cancel.cancelled() => PromptAnswer::Cancel,
            res = tokio::time::timeout(self.timeout, rx) => match res {
                Ok(Ok(answer)) => answer,
                _ => PromptAnswer::Cancel,
            },
        };
        self.lock().remove(&prompt_id);
        answer
    }

    /// Deliver an answer. Returns `false` if the id is unknown or expired.
    pub fn answer(&self, prompt_id: u64, answer: PromptAnswer) -> bool {
        match self.lock().remove(&prompt_id) {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<PromptAnswer>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}
```

In `crates/core/src/connect/mod.rs` insert between `pub mod prompt;` and `#[cfg(test)]`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use classify::{
    classify_io, classify_resolve_error, map_russh_error, network_error, sanitize_log_line,
};
pub use prompt::{EmitFn, HostKeyDecision, KbdPrompt, Prompt, PromptAnswer, PromptBroker};

use crate::error::CoreError;
use crate::known_hosts::KnownHosts;
use crate::model::{Host, LoginProfile};
use crate::secrets::SecretStore;
use crate::session::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    JumpHost,
    Tcp,
    HostKey,
    Auth,
    Channel,
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConnectEvent {
    Steps {
        steps: Vec<Step>,
    },
    StepStarted {
        step: Step,
    },
    StepDone {
        step: Step,
    },
    StepFailed {
        step: Step,
        error: CoreError,
    },
    /// Sanitized `ssh -v` style log line.
    Log {
        line: String,
    },
    Prompt {
        prompt_id: u64,
        prompt: Prompt,
    },
    Connected {
        session_id: SessionId,
    },
    Failed {
        error: CoreError,
    },
}

/// Build a `Log` event; the line is always sanitized.
pub fn log_line(line: impl Into<String>) -> ConnectEvent {
    ConnectEvent::Log {
        line: sanitize_log_line(&line.into()),
    }
}

#[derive(Debug, Clone)]
pub struct ConnectTarget {
    pub host: Host,
    pub profile: Option<LoginProfile>,
}

#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub target: ConnectTarget,
    pub jump: Option<ConnectTarget>,
    pub cols: u16,
    pub rows: u16,
}

pub struct ConnectDeps {
    pub known_hosts: KnownHosts,
    pub secrets: Arc<dyn SecretStore>,
    pub agent_sock: Option<PathBuf>,
}

/// Steps shown in the loading screen. `JumpHost` only with a jump host.
pub fn plan_steps(req: &ConnectRequest) -> Vec<Step> {
    let mut steps = Vec::with_capacity(6);
    if req.jump.is_some() {
        steps.push(Step::JumpHost);
    }
    steps.extend([
        Step::Tcp,
        Step::HostKey,
        Step::Auth,
        Step::Channel,
        Step::Shell,
    ]);
    steps
}
```

Note: `rename_all` on an enum only renames variant tags; `rename_all_fields = "camelCase"` (same convention as Tasks 1–7) makes the variant fields camelCase for the UI (`promptId`, `sessionId`, `keyPath`).

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p linascp-core --lib connect`
Expected: `test result: ok. 18 passed; 0 failed` (6 classify + 8 prompt + 4 mod)

- [ ] **Step 9: Commit**

```bash
git add crates/core/src/lib.rs crates/core/src/connect/mod.rs crates/core/src/connect/classify.rs crates/core/src/connect/prompt.rs
git commit -m "feat(core): add connect events, prompt broker and error classification

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Host key handler and authentication methods

**Files:**
- Modify: `crates/core/src/connect/mod.rs`
- Create: `crates/core/src/connect/handler.rs`
- Create: `crates/core/src/connect/auth.rs`
- Test: unit tests inside `handler.rs` and `auth.rs` (`cargo test -p linascp-core --lib connect`)

**Interfaces:**
- Consumes: Task 9 (`PromptBroker::ask_cancellable`, `EmitFn`, `Prompt`, `PromptAnswer`, `HostKeyDecision`, `KbdPrompt`, `log_line`, `ConnectTarget`, `map_russh_error`); `crate::known_hosts::{fingerprints, HostKeyInfo, HostKeyStatus, KnownHosts}` with `KnownHosts::{check, add, replace}`; `crate::secrets::{SecretKind, SecretOwner, SecretStore, MemorySecretStore}` (`SecretOwner: Copy + PartialEq + Debug`, `MemorySecretStore::new()`); `crate::model::{AuthConfig, AuthMethod, HostAuth, Host, LoginProfile}` (`AuthConfig: Clone`, `AuthMethod: Debug + PartialEq`); russh 0.64 (`client::Handler`, `Handle::{authenticate_password, authenticate_publickey, authenticate_publickey_with, authenticate_keyboard_interactive_start, authenticate_keyboard_interactive_respond, best_supported_rsa_hash}`, `keys::{decode_secret_key, PrivateKeyWithHashAlg, agent::client::AgentClient}`).
- Produces (`connect/handler.rs`, re-exported from `connect`):
  ```rust
  pub enum HostKeyAction { Accept, Trust, Replace }
  pub async fn decide_host_key<F, Fut>(status: HostKeyStatus, host: &str, port: u16, current: HostKeyInfo, ask: F) -> Result<HostKeyAction>
      where F: FnOnce(Prompt) -> Fut, Fut: Future<Output = PromptAnswer>;
  pub enum HandlerError { Ssh(russh::Error), Core(CoreError) }      // From<russh::Error>; CoreError: From<HandlerError>
  pub struct ClientHandler;                                          // impl russh::client::Handler<Error = HandlerError>
  impl ClientHandler { pub fn new(host: String, port: u16, known_hosts: Arc<KnownHosts>, broker: Arc<PromptBroker>, emit: EmitFn, cancel: CancellationToken) -> Self; }
  ```
  (`connect/auth.rs`):
  ```rust
  pub const MAX_ATTEMPTS: usize = 3;
  pub enum AuthOutcome { Success, Rejected { remaining: Vec<String> } }
  pub enum KbdStep { Success, Rejected { remaining: Vec<String> }, Info { name: String, instructions: String, prompts: Vec<KbdPrompt> } }
  #[async_trait] pub trait Authenticator: Send {
      async fn password(&mut self, user: &str, password: &SecretString) -> Result<AuthOutcome>;
      async fn publickey(&mut self, user: &str, key: PrivateKey) -> Result<AuthOutcome>;
      async fn agent(&mut self, user: &str, socket: &Path) -> Result<AuthOutcome>;
      async fn kbd_start(&mut self, user: &str) -> Result<KbdStep>;
      async fn kbd_respond(&mut self, responses: Vec<String>) -> Result<KbdStep>;
  }
  pub struct AuthContext<'a> { pub user, pub host, pub auth: AuthConfig, pub owner: SecretOwner, pub secrets: Arc<dyn SecretStore>,
                               pub agent_sock: Option<&'a Path>, pub broker: &'a PromptBroker, pub emit: &'a EmitFn, pub cancel: &'a CancellationToken }
  pub fn effective_user(target: &ConnectTarget) -> Result<String>;      // host.user, else profile.user, else $USER
  pub fn resolve_auth(target: &ConnectTarget) -> Result<(AuthConfig, SecretOwner)>;
  pub fn expand_tilde(path: &str, home: &Path) -> PathBuf;
  pub async fn authenticate(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()>;
  pub struct HandleAuthenticator<'a> { pub handle: &'a mut russh::client::Handle<ClientHandler> }   // impl Authenticator
  ```

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/connect/mod.rs` change the module declarations at the top to:

```rust
pub mod auth;
pub mod classify;
pub mod handler;
pub mod prompt;
```

Create `crates/core/src/connect/handler.rs` with only the tests:

```rust
//! russh client handler: host key verification against known_hosts.

#[cfg(test)]
mod tests {
    use super::*;

    fn info(sha: &str) -> HostKeyInfo {
        HostKeyInfo {
            algorithm: "ssh-ed25519".into(),
            sha256: sha.into(),
            md5: "MD5:00".into(),
        }
    }

    fn answer(decision: HostKeyDecision) -> PromptAnswer {
        PromptAnswer::HostKey { decision }
    }

    async fn decide(
        status: HostKeyStatus,
        reply: PromptAnswer,
    ) -> (Result<HostKeyAction>, Option<Prompt>) {
        let mut asked = None;
        let result = decide_host_key(status, "example.org", 2222, info("SHA256:new"), |p| {
            asked = Some(p);
            async move { reply }
        })
        .await;
        (result, asked)
    }

    #[tokio::test]
    async fn known_key_is_accepted_without_prompt() {
        let result = decide_host_key(
            HostKeyStatus::Known,
            "h",
            22,
            info("SHA256:a"),
            |_| async { panic!("must not ask for known keys") },
        )
        .await;
        assert_eq!(result.unwrap(), HostKeyAction::Accept);
    }

    #[tokio::test]
    async fn unknown_key_trust_and_accept_once() {
        let (r, asked) = decide(HostKeyStatus::Unknown, answer(HostKeyDecision::Trust)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Trust);
        assert_eq!(
            asked,
            Some(Prompt::HostKeyUnknown {
                host: "example.org".into(),
                port: 2222,
                info: info("SHA256:new")
            })
        );
        let (r, _) = decide(HostKeyStatus::Unknown, answer(HostKeyDecision::AcceptOnce)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Accept);
    }

    #[tokio::test]
    async fn unknown_key_rejected_on_reject_cancel_or_replace() {
        for reply in [
            answer(HostKeyDecision::Reject),
            answer(HostKeyDecision::Replace),
            PromptAnswer::Cancel,
            PromptAnswer::Secret {
                value: "x".into(),
                save: false,
            },
        ] {
            let (r, _) = decide(HostKeyStatus::Unknown, reply).await;
            assert!(matches!(r, Err(CoreError::HostKeyRejected { .. })));
        }
    }

    #[tokio::test]
    async fn changed_key_only_replace_is_accepted() {
        let changed = || HostKeyStatus::Changed {
            stored: info("SHA256:old"),
        };
        let (r, asked) = decide(changed(), answer(HostKeyDecision::Replace)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Replace);
        assert_eq!(
            asked,
            Some(Prompt::HostKeyChanged {
                host: "example.org".into(),
                port: 2222,
                stored: info("SHA256:old"),
                current: info("SHA256:new")
            })
        );
        for reply in [
            answer(HostKeyDecision::Trust),
            answer(HostKeyDecision::AcceptOnce),
            answer(HostKeyDecision::Reject),
            PromptAnswer::Cancel,
        ] {
            let (r, _) = decide(changed(), reply).await;
            assert!(matches!(r, Err(CoreError::HostKeyRejected { .. })));
        }
    }

    #[test]
    fn handler_error_converts_to_core_error() {
        let e: CoreError = HandlerError::Ssh(russh::Error::UnknownKey).into();
        assert!(matches!(e, CoreError::HostKeyRejected { .. }));
        let e: CoreError = HandlerError::Core(CoreError::Cancelled).into();
        assert!(matches!(e, CoreError::Cancelled));
    }
}
```

Create `crates/core/src/connect/auth.rs` with only the tests:

```rust
//! Execution of the configured SSH authentication method.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::ConnectEvent;
    use crate::model::{Host, LoginProfile};
    use crate::secrets::MemorySecretStore;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Ed25519 key encrypted with passphrase "secret" (bcrypt rounds = 1).
    const ENCRYPTED_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABAABAW/Yg
pZGaJaaKT237K+AAAAAQAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAIMOw7FedTlCVBfEC
b8cDep74mNhvctcJ5J7y5lAN3holAAAAkEB4i5HcxG1c0u9PQhTRmT+D8uFqhMe9mWovvm
LiGZPIOTuHs8y/yZ/vvT0MdmpUy2en4+HcFSW9/758edDtC/+VXfzTm8SFQMRYDdLo0Puq
GueqsyqKfkFI20/cf9dlN5VxtZHpt9Zy/YDMCcSgRoXdJBLKwEIWtLgOT4W/j++TpulttR
NKGOvMLy4gKjk9jA==
-----END OPENSSH PRIVATE KEY-----
";

    #[derive(Default)]
    struct FakeAuth {
        password_results: VecDeque<AuthOutcome>,
        passwords_seen: Vec<String>,
        publickey_calls: usize,
        kbd_steps: VecDeque<KbdStep>,
        kbd_responses: Vec<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl Authenticator for FakeAuth {
        async fn password(&mut self, _user: &str, password: &SecretString) -> Result<AuthOutcome> {
            self.passwords_seen
                .push(password.expose_secret().to_string());
            Ok(self.password_results.pop_front().expect("scripted result"))
        }
        async fn publickey(&mut self, _user: &str, _key: PrivateKey) -> Result<AuthOutcome> {
            self.publickey_calls += 1;
            Ok(AuthOutcome::Success)
        }
        async fn agent(&mut self, _user: &str, _socket: &Path) -> Result<AuthOutcome> {
            Ok(AuthOutcome::Rejected {
                remaining: vec!["publickey".into()],
            })
        }
        async fn kbd_start(&mut self, _user: &str) -> Result<KbdStep> {
            Ok(self.kbd_steps.pop_front().expect("scripted step"))
        }
        async fn kbd_respond(&mut self, responses: Vec<String>) -> Result<KbdStep> {
            self.kbd_responses.push(responses);
            Ok(self.kbd_steps.pop_front().expect("scripted step"))
        }
    }

    /// Answers prompts from a queue and records them.
    struct Harness {
        broker: Arc<PromptBroker>,
        emit: EmitFn,
        prompts: Arc<Mutex<Vec<Prompt>>>,
        secrets: Arc<MemorySecretStore>,
        cancel: CancellationToken,
        owner: SecretOwner,
    }

    impl Harness {
        fn new(answers: Vec<PromptAnswer>) -> Self {
            let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
            let prompts = Arc::new(Mutex::new(Vec::new()));
            let queue = Arc::new(Mutex::new(VecDeque::from(answers)));
            let (b, p) = (broker.clone(), prompts.clone());
            let emit: EmitFn = Arc::new(move |event| {
                if let ConnectEvent::Prompt { prompt_id, prompt } = event {
                    p.lock().unwrap().push(prompt);
                    let answer = queue
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or(PromptAnswer::Cancel);
                    b.answer(prompt_id, answer);
                }
            });
            Self {
                broker,
                emit,
                prompts,
                secrets: Arc::new(MemorySecretStore::new()),
                cancel: CancellationToken::new(),
                owner: SecretOwner::Host(uuid::Uuid::new_v4()),
            }
        }

        fn ctx(&self, method: AuthMethod, key_path: Option<String>) -> AuthContext<'_> {
            AuthContext {
                user: "alice".into(),
                host: "example.org".into(),
                auth: AuthConfig {
                    method,
                    key_path,
                    has_secret: false,
                },
                owner: self.owner,
                secrets: self.secrets.clone(),
                agent_sock: None,
                broker: &self.broker,
                emit: &self.emit,
                cancel: &self.cancel,
            }
        }

        fn prompts(&self) -> Vec<Prompt> {
            self.prompts.lock().unwrap().clone()
        }
    }

    fn secret(value: &str, save: bool) -> PromptAnswer {
        PromptAnswer::Secret {
            value: value.into(),
            save,
        }
    }

    fn rejected(methods: &[&str]) -> AuthOutcome {
        AuthOutcome::Rejected {
            remaining: methods.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn effective_user_prefers_host_then_profile() {
        let mut host = Host::new("web", "web.example.org");
        let profile = LoginProfile {
            id: uuid::Uuid::new_v4(),
            name: "ops".into(),
            user: "ops".into(),
            auth: AuthConfig {
                method: AuthMethod::Agent,
                key_path: None,
                has_secret: false,
            },
            created_at: 0,
            updated_at: 0,
        };
        let mut target = ConnectTarget {
            host: host.clone(),
            profile: Some(profile.clone()),
        };
        assert_eq!(effective_user(&target).unwrap(), "ops");
        host.user = Some("root".into());
        target.host = host;
        assert_eq!(effective_user(&target).unwrap(), "root");
    }

    #[test]
    fn resolve_auth_owner_is_profile_or_host() {
        let mut host = Host::new("web", "web.example.org");
        let target = ConnectTarget {
            host: host.clone(),
            profile: None,
        };
        let (auth, owner) = resolve_auth(&target).unwrap();
        assert_eq!(auth.method, AuthMethod::Password);
        assert_eq!(owner, SecretOwner::Host(host.id));

        let profile = LoginProfile {
            id: uuid::Uuid::new_v4(),
            name: "ops".into(),
            user: "ops".into(),
            auth: AuthConfig {
                method: AuthMethod::Key,
                key_path: Some("~/.ssh/id_ed25519".into()),
                has_secret: true,
            },
            created_at: 0,
            updated_at: 0,
        };
        host.auth = HostAuth::Profile {
            profile_id: profile.id,
        };
        let target = ConnectTarget {
            host: host.clone(),
            profile: Some(profile.clone()),
        };
        let (auth, owner) = resolve_auth(&target).unwrap();
        assert_eq!(auth.method, AuthMethod::Key);
        assert_eq!(owner, SecretOwner::Profile(profile.id));

        let missing = ConnectTarget {
            host,
            profile: None,
        };
        assert!(matches!(
            resolve_auth(&missing),
            Err(CoreError::Config { .. })
        ));
    }

    #[test]
    fn expand_tilde_uses_home() {
        let home = Path::new("/home/u");
        assert_eq!(
            expand_tilde("~/.ssh/id", home),
            PathBuf::from("/home/u/.ssh/id")
        );
        assert_eq!(expand_tilde("/etc/key", home), PathBuf::from("/etc/key"));
    }

    #[tokio::test]
    async fn password_from_store_needs_no_prompt() {
        let h = Harness::new(vec![]);
        h.secrets
            .set(
                h.owner,
                SecretKind::Password,
                &SecretString::from("stored".to_string()),
            )
            .unwrap();
        let mut fake = FakeAuth {
            password_results: VecDeque::from([AuthOutcome::Success]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::Password, None))
            .await
            .unwrap();
        assert_eq!(fake.passwords_seen, vec!["stored"]);
        assert!(h.prompts().is_empty());
    }

    #[tokio::test]
    async fn prompted_password_is_saved_on_success() {
        let h = Harness::new(vec![secret("typed", true)]);
        let mut fake = FakeAuth {
            password_results: VecDeque::from([AuthOutcome::Success]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::Password, None))
            .await
            .unwrap();
        assert_eq!(
            h.prompts(),
            vec![Prompt::Password {
                user: "alice".into(),
                host: "example.org".into(),
                retry: false
            }]
        );
        let saved = h
            .secrets
            .get(h.owner, SecretKind::Password)
            .unwrap()
            .unwrap();
        assert_eq!(saved.expose_secret(), "typed");
    }

    #[tokio::test]
    async fn password_gives_up_after_three_attempts() {
        let h = Harness::new(vec![secret("b", true), secret("c", false)]);
        h.secrets
            .set(
                h.owner,
                SecretKind::Password,
                &SecretString::from("a".to_string()),
            )
            .unwrap();
        let remaining = ["publickey", "password"];
        let mut fake = FakeAuth {
            password_results: VecDeque::from([
                rejected(&remaining),
                rejected(&remaining),
                rejected(&remaining),
            ]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None))
            .await
            .unwrap_err();
        match err {
            CoreError::Auth {
                rejected_methods, ..
            } => assert_eq!(rejected_methods, vec!["publickey", "password"]),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(fake.passwords_seen, vec!["a", "b", "c"]);
        let retries: Vec<bool> = h
            .prompts()
            .into_iter()
            .map(|p| match p {
                Prompt::Password { retry, .. } => retry,
                other => panic!("unexpected prompt {other:?}"),
            })
            .collect();
        assert_eq!(retries, vec![true, true]);
        // Failed secrets are never persisted.
        assert_eq!(
            h.secrets
                .get(h.owner, SecretKind::Password)
                .unwrap()
                .unwrap()
                .expose_secret(),
            "a"
        );
    }

    #[tokio::test]
    async fn password_stops_when_server_drops_password_method() {
        let h = Harness::new(vec![secret("x", false)]);
        let mut fake = FakeAuth {
            password_results: VecDeque::from([rejected(&["publickey"])]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Auth { .. }));
        assert_eq!(fake.passwords_seen.len(), 1);
    }

    #[tokio::test]
    async fn password_prompt_cancel_aborts() {
        let h = Harness::new(vec![PromptAnswer::Cancel]);
        let mut fake = FakeAuth::default();
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Cancelled));
    }

    #[tokio::test]
    async fn encrypted_key_retries_passphrase_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("id_ed25519");
        std::fs::write(&key_path, ENCRYPTED_KEY).unwrap();
        let h = Harness::new(vec![secret("wrong", true), secret("secret", true)]);
        let mut fake = FakeAuth::default();
        authenticate(
            &mut fake,
            &h.ctx(AuthMethod::Key, Some(key_path.display().to_string())),
        )
        .await
        .unwrap();
        assert_eq!(fake.publickey_calls, 1);
        assert_eq!(h.prompts().len(), 2);
        assert!(matches!(h.prompts()[0], Prompt::Passphrase { .. }));
        let saved = h
            .secrets
            .get(h.owner, SecretKind::Passphrase)
            .unwrap()
            .unwrap();
        assert_eq!(saved.expose_secret(), "secret");
    }

    #[tokio::test]
    async fn missing_key_file_is_config_error() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth::default();
        let err = authenticate(
            &mut fake,
            &h.ctx(AuthMethod::Key, Some("/nonexistent/linascp/key".into())),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }));
    }

    #[tokio::test]
    async fn agent_without_socket_fails() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth::default();
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Agent, None))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Auth { .. }));
    }

    #[tokio::test]
    async fn keyboard_interactive_round_trip() {
        let h = Harness::new(vec![PromptAnswer::Responses {
            values: vec!["123456".into()],
        }]);
        let mut fake = FakeAuth {
            kbd_steps: VecDeque::from([
                KbdStep::Info {
                    name: String::new(),
                    instructions: String::new(),
                    prompts: vec![],
                },
                KbdStep::Info {
                    name: "2FA".into(),
                    instructions: "Enter code".into(),
                    prompts: vec![KbdPrompt {
                        text: "Code: ".into(),
                        echo: true,
                    }],
                },
                KbdStep::Success,
            ]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::KeyboardInteractive, None))
            .await
            .unwrap();
        assert_eq!(
            fake.kbd_responses,
            vec![vec![], vec!["123456".to_string()]]
        );
        assert_eq!(h.prompts().len(), 1);
    }

    #[tokio::test]
    async fn keyboard_interactive_failure_reports_methods() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth {
            kbd_steps: VecDeque::from([KbdStep::Rejected {
                remaining: vec!["keyboard-interactive".into()],
            }]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::KeyboardInteractive, None))
            .await
            .unwrap_err();
        match err {
            CoreError::Auth {
                rejected_methods, ..
            } => assert_eq!(rejected_methods, vec!["keyboard-interactive"]),
            other => panic!("unexpected {other:?}"),
        }
    }
}
```

The embedded key is a throwaway test key (generated with `ssh-keygen -t ed25519 -a 1 -N secret`); it is not used anywhere else.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core --lib connect`
Expected: compilation fails with `cannot find function 'decide_host_key'`, `cannot find type 'HandlerError'`, `cannot find trait 'Authenticator'`, `cannot find function 'authenticate'`, etc.

- [ ] **Step 3: Implement handler.rs, auth.rs and the re-exports**

In `crates/core/src/connect/handler.rs` insert between the `//!` line and `#[cfg(test)]`:

```rust
use std::future::Future;
use std::sync::Arc;

use russh::client;
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use tokio_util::sync::CancellationToken;

use super::classify::map_russh_error;
use super::log_line;
use super::prompt::{EmitFn, HostKeyDecision, Prompt, PromptAnswer, PromptBroker};
use crate::error::{CoreError, Result};
use crate::known_hosts::{fingerprints, HostKeyInfo, HostKeyStatus, KnownHosts};

/// What to do with a server key after the user (or known_hosts) decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyAction {
    /// Continue without touching known_hosts (known key or "accept once").
    Accept,
    /// Continue and append the key (hashed) to known_hosts.
    Trust,
    /// Continue and replace the stored key of the same algorithm.
    Replace,
}

fn rejected(message: &str) -> CoreError {
    CoreError::HostKeyRejected {
        message: message.to_string(),
    }
}

/// Pure decision logic. `ask` is only invoked for unknown or changed keys.
/// Fail-closed: anything but a valid, explicit acceptance is a rejection.
pub async fn decide_host_key<F, Fut>(
    status: HostKeyStatus,
    host: &str,
    port: u16,
    current: HostKeyInfo,
    ask: F,
) -> Result<HostKeyAction>
where
    F: FnOnce(Prompt) -> Fut,
    Fut: Future<Output = PromptAnswer>,
{
    match status {
        HostKeyStatus::Known => Ok(HostKeyAction::Accept),
        HostKeyStatus::Unknown => {
            let answer = ask(Prompt::HostKeyUnknown {
                host: host.to_string(),
                port,
                info: current,
            })
            .await;
            match answer {
                PromptAnswer::HostKey {
                    decision: HostKeyDecision::AcceptOnce,
                } => Ok(HostKeyAction::Accept),
                PromptAnswer::HostKey {
                    decision: HostKeyDecision::Trust,
                } => Ok(HostKeyAction::Trust),
                _ => Err(rejected("unknown host key was not accepted")),
            }
        }
        HostKeyStatus::Changed { stored } => {
            let answer = ask(Prompt::HostKeyChanged {
                host: host.to_string(),
                port,
                stored,
                current,
            })
            .await;
            match answer {
                PromptAnswer::HostKey {
                    decision: HostKeyDecision::Replace,
                } => Ok(HostKeyAction::Replace),
                _ => Err(rejected(
                    "REMOTE HOST IDENTIFICATION HAS CHANGED; connection refused",
                )),
            }
        }
    }
}

/// Error type of the russh handler: either a transport error or our own.
#[derive(Debug)]
pub enum HandlerError {
    Ssh(russh::Error),
    Core(CoreError),
}

impl From<russh::Error> for HandlerError {
    fn from(err: russh::Error) -> Self {
        HandlerError::Ssh(err)
    }
}

impl From<HandlerError> for CoreError {
    fn from(err: HandlerError) -> Self {
        match err {
            HandlerError::Ssh(e) => map_russh_error(e),
            HandlerError::Core(e) => e,
        }
    }
}

/// russh handler for one SSH hop (jump host or target).
pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    cancel: CancellationToken,
}

impl ClientHandler {
    pub fn new(
        host: String,
        port: u16,
        known_hosts: Arc<KnownHosts>,
        broker: Arc<PromptBroker>,
        emit: EmitFn,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            host,
            port,
            known_hosts,
            broker,
            emit,
            cancel,
        }
    }

    async fn verify(&self, key: &PublicKey) -> Result<()> {
        let info = fingerprints(key);
        (self.emit)(log_line(format!(
            "debug1: Server host key: {} {}",
            info.algorithm, info.sha256
        )));
        let status = self.known_hosts.check(&self.host, self.port, key)?;
        let status_text = match &status {
            HostKeyStatus::Known => "known",
            HostKeyStatus::Unknown => "unknown",
            HostKeyStatus::Changed { .. } => "CHANGED",
        };
        (self.emit)(log_line(format!(
            "debug1: Host '{}' key is {status_text}",
            self.host
        )));
        let action = decide_host_key(status, &self.host, self.port, info, |prompt| {
            self.broker
                .ask_cancellable(&self.emit, prompt, &self.cancel)
        })
        .await?;
        match action {
            HostKeyAction::Accept => {}
            HostKeyAction::Trust => {
                self.known_hosts.add(&self.host, self.port, key, true)?;
                (self.emit)(log_line(format!(
                    "Warning: Permanently added '{}' to the list of known hosts.",
                    self.host
                )));
            }
            HostKeyAction::Replace => {
                self.known_hosts.replace(&self.host, self.port, key)?;
                (self.emit)(log_line(format!(
                    "debug1: Replaced host key for '{}' in known_hosts",
                    self.host
                )));
            }
        }
        Ok(())
    }
}

impl client::Handler for ClientHandler {
    type Error = HandlerError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                self.verify(key).await.map_err(HandlerError::Core)?;
                Ok(true)
            }
            PublicKeyOrCertificate::Certificate(_) => Err(HandlerError::Core(rejected(
                "host certificates are not supported",
            ))),
        }
    }
}
```

Why `HandlerError::Core` instead of `Ok(false)`: russh 0.64 propagates handler errors out of `client::connect_stream` (via the session join handle), so the pipeline receives the exact `CoreError` (`HostKeyRejected`, `Cancelled`, keyring/IO errors) instead of a generic `russh::Error::UnknownKey`.

In `crates/core/src/connect/auth.rs` insert between the `//!` line and `#[cfg(test)]`:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::AgentClient;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::MethodSet;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

use super::classify::map_russh_error;
use super::handler::ClientHandler;
use super::prompt::{EmitFn, KbdPrompt, Prompt, PromptAnswer, PromptBroker};
use super::{log_line, ConnectTarget};
use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, AuthMethod, HostAuth};
use crate::secrets::{SecretKind, SecretOwner, SecretStore};

/// Maximum password / passphrase attempts before giving up.
pub const MAX_ATTEMPTS: usize = 3;
/// Upper bound for keyboard-interactive info request rounds.
const MAX_KBD_ROUNDS: usize = 16;

/// Result of a single authentication request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Success,
    /// Rejected; `remaining` = methods the server still accepts.
    Rejected {
        remaining: Vec<String>,
    },
}

/// One step of a keyboard-interactive exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbdStep {
    Success,
    Rejected {
        remaining: Vec<String>,
    },
    Info {
        name: String,
        instructions: String,
        prompts: Vec<KbdPrompt>,
    },
}

/// Transport abstraction so the auth flow is testable without a server.
#[async_trait::async_trait]
pub trait Authenticator: Send {
    async fn password(&mut self, user: &str, password: &SecretString) -> Result<AuthOutcome>;
    async fn publickey(&mut self, user: &str, key: PrivateKey) -> Result<AuthOutcome>;
    async fn agent(&mut self, user: &str, socket: &Path) -> Result<AuthOutcome>;
    async fn kbd_start(&mut self, user: &str) -> Result<KbdStep>;
    async fn kbd_respond(&mut self, responses: Vec<String>) -> Result<KbdStep>;
}

/// Everything the auth flow needs besides the transport.
pub struct AuthContext<'a> {
    pub user: String,
    pub host: String,
    pub auth: AuthConfig,
    pub owner: SecretOwner,
    pub secrets: Arc<dyn SecretStore>,
    pub agent_sock: Option<&'a Path>,
    pub broker: &'a PromptBroker,
    pub emit: &'a EmitFn,
    pub cancel: &'a CancellationToken,
}

/// User name: host override, else login profile, else local `$USER`.
pub fn effective_user(target: &ConnectTarget) -> Result<String> {
    let non_empty = |s: &String| !s.trim().is_empty();
    if let Some(user) = target.host.user.clone().filter(non_empty) {
        return Ok(user);
    }
    if let Some(user) = target
        .profile
        .as_ref()
        .map(|p| p.user.clone())
        .filter(non_empty)
    {
        return Ok(user);
    }
    std::env::var("USER")
        .ok()
        .filter(non_empty)
        .ok_or_else(|| CoreError::Config {
            message: format!("no user configured for host '{}'", target.host.name),
        })
}

/// Auth settings and secret owner (profile or host) for a target.
pub fn resolve_auth(target: &ConnectTarget) -> Result<(AuthConfig, SecretOwner)> {
    match &target.host.auth {
        HostAuth::Custom { auth } => Ok((auth.clone(), SecretOwner::Host(target.host.id))),
        HostAuth::Profile { profile_id } => match &target.profile {
            Some(profile) if profile.id == *profile_id => {
                Ok((profile.auth.clone(), SecretOwner::Profile(profile.id)))
            }
            _ => Err(CoreError::Config {
                message: format!("login profile {profile_id} not found"),
            }),
        },
    }
}

/// Expand a leading `~/` against `home`.
pub fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

fn method_names(set: &MethodSet) -> Vec<String> {
    set.iter().map(String::from).collect()
}

fn auth_error(remaining: Vec<String>, fallback: &str, message: &str) -> CoreError {
    let rejected_methods = if remaining.is_empty() {
        vec![fallback.to_string()]
    } else {
        remaining
    };
    CoreError::Auth {
        rejected_methods,
        message: message.to_string(),
    }
}

impl AuthContext<'_> {
    fn log(&self, line: String) {
        (self.emit)(log_line(line));
    }

    /// Read a stored secret. `SecretStore` calls may block (D-Bus keyring),
    /// so they run on the blocking pool.
    async fn stored_secret(&self, kind: SecretKind) -> Option<SecretString> {
        let secrets = self.secrets.clone();
        let owner = self.owner;
        let result = tokio::task::spawn_blocking(move || secrets.get(owner, kind))
            .await
            .unwrap_or_else(|e| {
                Err(CoreError::Internal {
                    message: e.to_string(),
                })
            });
        match result {
            Ok(secret) => secret,
            Err(e) => {
                self.log(format!("debug1: keyring unavailable: {e}"));
                None
            }
        }
    }

    /// Persist a secret that the server just accepted (best effort).
    async fn save_secret(&self, kind: SecretKind, value: SecretString) {
        let secrets = self.secrets.clone();
        let owner = self.owner;
        let result = tokio::task::spawn_blocking(move || secrets.set(owner, kind, &value))
            .await
            .unwrap_or_else(|e| {
                Err(CoreError::Internal {
                    message: e.to_string(),
                })
            });
        if let Err(e) = result {
            self.log(format!("debug1: could not save secret in keyring: {e}"));
        }
    }

    /// Ask for a password/passphrase; a cancelled prompt aborts the connect.
    async fn ask_secret(&self, prompt: Prompt) -> Result<(SecretString, bool)> {
        match self
            .broker
            .ask_cancellable(self.emit, prompt, self.cancel)
            .await
        {
            PromptAnswer::Secret { value, save } => Ok((SecretString::from(value), save)),
            _ => Err(CoreError::Cancelled),
        }
    }
}

/// Run the configured method once (with retries where applicable).
pub async fn authenticate(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    ctx.log(format!(
        "debug1: Authenticating as '{}' using {:?}",
        ctx.user, ctx.auth.method
    ));
    let result = match ctx.auth.method {
        AuthMethod::Password => password_flow(auth, ctx).await,
        AuthMethod::Key => key_flow(auth, ctx).await,
        AuthMethod::Agent => agent_flow(auth, ctx).await,
        AuthMethod::KeyboardInteractive => kbd_flow(auth, ctx).await,
    };
    if result.is_ok() {
        ctx.log(format!("debug1: Authenticated as '{}'", ctx.user));
    }
    result
}

async fn password_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    let mut stored = ctx.stored_secret(SecretKind::Password).await;
    let mut remaining = Vec::new();
    for attempt in 0..MAX_ATTEMPTS {
        let (password, save) = match stored.take() {
            Some(secret) => (secret, false),
            None => {
                ctx.ask_secret(Prompt::Password {
                    user: ctx.user.clone(),
                    host: ctx.host.clone(),
                    retry: attempt > 0,
                })
                .await?
            }
        };
        match auth.password(&ctx.user, &password).await? {
            AuthOutcome::Success => {
                if save {
                    ctx.save_secret(SecretKind::Password, password).await;
                }
                return Ok(());
            }
            AuthOutcome::Rejected { remaining: r } => {
                ctx.log("debug1: Password authentication rejected".to_string());
                remaining = r;
                if !remaining.is_empty() && !remaining.iter().any(|m| m == "password") {
                    break;
                }
            }
        }
    }
    Err(auth_error(
        remaining,
        "password",
        "password authentication failed",
    ))
}

fn decode_key(
    text: &str,
    passphrase: Option<&str>,
) -> std::result::Result<PrivateKey, russh::keys::Error> {
    russh::keys::decode_secret_key(text, passphrase)
}

async fn load_key(ctx: &AuthContext<'_>, key_path: &str) -> Result<PrivateKey> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = expand_tilde(key_path, Path::new(&home));
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| CoreError::Config {
            message: format!("cannot read key file {}: {e}", path.display()),
        })?;
    match decode_key(&text, None) {
        Ok(key) => return Ok(key),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => {
            return Err(CoreError::Config {
                message: format!("invalid key file {}: {e}", path.display()),
            });
        }
    }
    let mut stored = ctx.stored_secret(SecretKind::Passphrase).await;
    for _ in 0..MAX_ATTEMPTS {
        let (passphrase, save) = match stored.take() {
            Some(secret) => (secret, false),
            None => {
                ctx.ask_secret(Prompt::Passphrase {
                    key_path: key_path.to_string(),
                })
                .await?
            }
        };
        match decode_key(&text, Some(passphrase.expose_secret())) {
            Ok(key) => {
                if save {
                    ctx.save_secret(SecretKind::Passphrase, passphrase).await;
                }
                return Ok(key);
            }
            Err(_) => ctx.log("debug1: Wrong passphrase for key".to_string()),
        }
    }
    Err(CoreError::Auth {
        rejected_methods: vec!["publickey".to_string()],
        message: format!("could not decrypt key {key_path}"),
    })
}

async fn key_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    let key_path = ctx.auth.key_path.clone().ok_or_else(|| CoreError::Config {
        message: "no key file configured".to_string(),
    })?;
    let key = load_key(ctx, &key_path).await?;
    ctx.log(format!("debug1: Offering public key: {key_path}"));
    match auth.publickey(&ctx.user, key).await? {
        AuthOutcome::Success => Ok(()),
        AuthOutcome::Rejected { remaining } => Err(auth_error(
            remaining,
            "publickey",
            "public key was rejected",
        )),
    }
}

async fn agent_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    let socket = ctx.agent_sock.ok_or_else(|| CoreError::Auth {
        rejected_methods: vec!["publickey".to_string()],
        message: "no ssh-agent available (SSH_AUTH_SOCK not set)".to_string(),
    })?;
    match auth.agent(&ctx.user, socket).await? {
        AuthOutcome::Success => Ok(()),
        AuthOutcome::Rejected { remaining } => Err(auth_error(
            remaining,
            "publickey",
            "no agent identity was accepted",
        )),
    }
}

async fn kbd_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    let mut step = auth.kbd_start(&ctx.user).await?;
    for _ in 0..MAX_KBD_ROUNDS {
        match step {
            KbdStep::Success => return Ok(()),
            KbdStep::Rejected { remaining } => {
                return Err(auth_error(
                    remaining,
                    "keyboard-interactive",
                    "keyboard-interactive authentication failed",
                ));
            }
            KbdStep::Info {
                name,
                instructions,
                prompts,
            } => {
                let responses = if prompts.is_empty() {
                    Vec::new()
                } else {
                    let expected = prompts.len();
                    match ctx
                        .broker
                        .ask_cancellable(
                            ctx.emit,
                            Prompt::KeyboardInteractive {
                                name,
                                instructions,
                                prompts,
                            },
                            ctx.cancel,
                        )
                        .await
                    {
                        PromptAnswer::Responses { values } if values.len() == expected => values,
                        PromptAnswer::Responses { .. } => {
                            return Err(CoreError::Auth {
                                rejected_methods: vec!["keyboard-interactive".to_string()],
                                message: "wrong number of responses".to_string(),
                            });
                        }
                        _ => return Err(CoreError::Cancelled),
                    }
                };
                step = auth.kbd_respond(responses).await?;
            }
        }
    }
    Err(CoreError::Auth {
        rejected_methods: vec!["keyboard-interactive".to_string()],
        message: "too many keyboard-interactive rounds".to_string(),
    })
}

/// Real transport: a connected russh client handle.
pub struct HandleAuthenticator<'a> {
    pub handle: &'a mut client::Handle<ClientHandler>,
}

fn outcome(result: client::AuthResult) -> AuthOutcome {
    match result {
        client::AuthResult::Success => AuthOutcome::Success,
        client::AuthResult::Failure {
            remaining_methods, ..
        } => AuthOutcome::Rejected {
            remaining: method_names(&remaining_methods),
        },
    }
}

fn kbd_step(response: KeyboardInteractiveAuthResponse) -> KbdStep {
    match response {
        KeyboardInteractiveAuthResponse::Success => KbdStep::Success,
        KeyboardInteractiveAuthResponse::Failure {
            remaining_methods, ..
        } => KbdStep::Rejected {
            remaining: method_names(&remaining_methods),
        },
        KeyboardInteractiveAuthResponse::InfoRequest {
            name,
            instructions,
            prompts,
        } => KbdStep::Info {
            name,
            instructions,
            prompts: prompts
                .into_iter()
                .map(|p| KbdPrompt {
                    text: p.prompt,
                    echo: p.echo,
                })
                .collect(),
        },
    }
}

impl HandleAuthenticator<'_> {
    async fn rsa_hash(&self) -> Result<Option<HashAlg>> {
        let best = self
            .handle
            .best_supported_rsa_hash()
            .await
            .map_err(map_russh_error)?;
        // Server did not announce server-sig-algs: assume rsa-sha2-256.
        Ok(best.unwrap_or(Some(HashAlg::Sha256)))
    }
}

#[async_trait::async_trait]
impl Authenticator for HandleAuthenticator<'_> {
    async fn password(&mut self, user: &str, password: &SecretString) -> Result<AuthOutcome> {
        self.handle
            .authenticate_password(user, password.expose_secret())
            .await
            .map(outcome)
            .map_err(map_russh_error)
    }

    async fn publickey(&mut self, user: &str, key: PrivateKey) -> Result<AuthOutcome> {
        let hash = if key.algorithm().is_rsa() {
            self.rsa_hash().await?
        } else {
            None
        };
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
        self.handle
            .authenticate_publickey(user, key)
            .await
            .map(outcome)
            .map_err(map_russh_error)
    }

    async fn agent(&mut self, user: &str, socket: &Path) -> Result<AuthOutcome> {
        let agent_error = |e: russh::keys::Error| CoreError::Auth {
            rejected_methods: vec!["publickey".to_string()],
            message: format!("ssh-agent: {e}"),
        };
        let mut agent = AgentClient::connect_uds(socket)
            .await
            .map_err(agent_error)?;
        let identities = agent.request_identities().await.map_err(agent_error)?;
        let mut last = AuthOutcome::Rejected {
            remaining: Vec::new(),
        };
        for identity in identities {
            let public = identity.public_key().into_owned();
            let hash = if public.algorithm().is_rsa() {
                self.rsa_hash().await?
            } else {
                None
            };
            let result = self
                .handle
                .authenticate_publickey_with(user, public, hash, &mut agent)
                .await
                .map_err(|e| CoreError::Auth {
                    rejected_methods: vec!["publickey".to_string()],
                    message: format!("ssh-agent: {e}"),
                })?;
            last = outcome(result);
            if last == AuthOutcome::Success {
                break;
            }
        }
        Ok(last)
    }

    async fn kbd_start(&mut self, user: &str) -> Result<KbdStep> {
        self.handle
            .authenticate_keyboard_interactive_start(user, None::<String>)
            .await
            .map(kbd_step)
            .map_err(map_russh_error)
    }

    async fn kbd_respond(&mut self, responses: Vec<String>) -> Result<KbdStep> {
        self.handle
            .authenticate_keyboard_interactive_respond(responses)
            .await
            .map(kbd_step)
            .map_err(map_russh_error)
    }
}
```

In `crates/core/src/connect/mod.rs`, directly below the existing `pub use prompt::{...};` line add:

```rust
pub use handler::{decide_host_key, ClientHandler, HandlerError, HostKeyAction};
```

Secret handling notes: `SecretStore` calls are blocking (D-Bus keyring) and therefore always run via `tokio::task::spawn_blocking` (hence `AuthContext.secrets` is an `Arc<dyn SecretStore>`); prompt answers are moved into `SecretString` immediately; a secret is persisted only after the server accepted it and only if the user ticked "save"; keyring failures degrade to prompting (logged without the secret, spec §5 "Speichern derzeit nicht möglich" is shown by the UI).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p linascp-core --lib connect`
Expected: `test result: ok. 36 passed; 0 failed` (18 from Task 9 + 5 handler + 13 auth)

- [ ] **Step 4b: Review-focus test — locked/unavailable keyring**

Append inside the existing `#[cfg(test)] mod tests` of `crates/core/src/connect/auth.rs` (it reuses `Harness`, `FakeAuth`, `secret`, `authenticate`):

```rust
    struct LockedKeyring;

    impl crate::secrets::SecretStore for LockedKeyring {
        fn get(
            &self,
            _: SecretOwner,
            _: SecretKind,
        ) -> std::result::Result<Option<SecretString>, CoreError> {
            Err(CoreError::Keyring { message: "collection is locked".into() })
        }
        fn set(&self, _: SecretOwner, _: SecretKind, _: &SecretString) -> std::result::Result<(), CoreError> {
            Err(CoreError::Keyring { message: "collection is locked".into() })
        }
        fn delete(&self, _: SecretOwner, _: SecretKind) -> std::result::Result<(), CoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn locked_keyring_falls_back_to_prompt_and_still_authenticates() {
        let h = Harness::new(vec![secret("typed", true)]);
        let mut ctx = h.ctx(AuthMethod::Password, None);
        ctx.secrets = Arc::new(LockedKeyring);
        let mut fake = FakeAuth {
            password_results: VecDeque::from([AuthOutcome::Success]),
            ..Default::default()
        };
        authenticate(&mut fake, &ctx).await.unwrap();
        assert_eq!(fake.passwords_seen, vec!["typed"]);
        assert_eq!(h.prompts().len(), 1);
    }
```

Run: `cargo test -p linascp-core --lib connect`
Expected: `37 passed` — a locked keyring must never abort the connection (spec §5); saving silently fails and is only logged.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/connect/mod.rs crates/core/src/connect/handler.rs crates/core/src/connect/auth.rs
git commit -m "feat(core): add host key verification handler and SSH auth methods

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: SshSession and the connect pipeline (connect_ssh, test_connection)

**Files:**
- Create: `crates/core/src/connect/ssh_session.rs`
- Modify: `crates/core/src/connect/mod.rs`
- Test: `pipeline_tests` module in `crates/core/src/connect/mod.rs` (network-free; real SSH is covered in Task 12)

**Interfaces:**
- Consumes: Tasks 8–10 (`Session`, `SessionEvent`, `OutputSink`, `ClientHandler`, `auth::{effective_user, resolve_auth, AuthContext, HandleAuthenticator, authenticate}`, classify helpers, `PromptBroker`); russh 0.64 (`client::{Config, connect_stream, Handle}`, `Handle::{channel_open_session, channel_open_direct_tcpip, disconnect}`, `Channel::{request_pty, set_env, request_shell, data, split, into_stream}`, `ChannelWriteHalf::{data, window_change, close}`, `ChannelReadHalf::wait`, `ChannelMsg`, `Disconnect`).
- Produces:
  ```rust
  pub struct SshSession;   // impl Session; fields private
  impl SshSession { pub(crate) fn start(channel: russh::Channel<russh::client::Msg>, handle: russh::client::Handle<ClientHandler>,
                                         jump: Option<russh::client::Handle<ClientHandler>>, sink: OutputSink) -> Self; }
  pub async fn connect_ssh(req: ConnectRequest, deps: ConnectDeps, broker: Arc<PromptBroker>,
                           emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>, sink: OutputSink,
                           cancel: CancellationToken) -> Result<SshSession>;
  pub async fn test_connection(req: ConnectRequest, deps: ConnectDeps, broker: Arc<PromptBroker>,
                               emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>, cancel: CancellationToken) -> Result<()>;
  ```
  Behaviour: emits `Steps` first, then `StepStarted`/`StepDone`/`StepFailed` per step plus sanitized `Log` lines; on error emits `Failed { error }` last. `connect_ssh` does NOT emit `Connected` — the caller registers the session under its own `SessionId` and emits `Connected { session_id }`. Prompts are published through the `emit` passed to `connect_ssh` (via `broker.ask_cancellable`), so one global broker serves all connects. `test_connection` emits `Steps` without `Channel`/`Shell` and disconnects after `Auth`.

- [ ] **Step 1: Write the failing pipeline tests**

Append to the end of `crates/core/src/connect/mod.rs`:

```rust
#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use crate::secrets::MemorySecretStore;
    use std::sync::Mutex;

    struct Recorder {
        events: Arc<Mutex<Vec<ConnectEvent>>>,
        emit: EmitFn,
    }

    fn recorder() -> Recorder {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let emit: EmitFn = Arc::new(move |event| e2.lock().unwrap().push(event));
        Recorder { events, emit }
    }

    fn deps(dir: &std::path::Path) -> ConnectDeps {
        ConnectDeps {
            known_hosts: KnownHosts::new(dir.join("known_hosts")),
            secrets: Arc::new(MemorySecretStore::new()),
            agent_sock: None,
        }
    }

    fn request(address: &str, port: u16) -> ConnectRequest {
        let mut host = Host::new("t", address);
        host.port = port;
        host.timeout_s = 5;
        ConnectRequest {
            target: ConnectTarget {
                host,
                profile: None,
            },
            jump: None,
            cols: 80,
            rows: 24,
        }
    }

    fn noop_sink() -> OutputSink {
        Arc::new(|_| {})
    }

    async fn run(
        req: ConnectRequest,
        cancel: CancellationToken,
    ) -> (Result<SshSession>, Vec<ConnectEvent>) {
        let dir = tempfile::tempdir().unwrap();
        let rec = recorder();
        let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
        let result = connect_ssh(
            req,
            deps(dir.path()),
            broker,
            rec.emit.clone(),
            noop_sink(),
            cancel,
        )
        .await;
        let events = rec.events.lock().unwrap().clone();
        (result, events)
    }

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn refused_port_is_classified_and_reported() {
        let (result, events) =
            run(request("127.0.0.1", free_port()), CancellationToken::new()).await;
        assert!(matches!(
            result,
            Err(CoreError::Network {
                kind: NetworkKind::Refused,
                ..
            })
        ));
        assert!(matches!(&events[0], ConnectEvent::Steps { steps } if steps[0] == Step::Tcp));
        assert!(matches!(
            events[1],
            ConnectEvent::StepStarted { step: Step::Tcp }
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            ConnectEvent::StepFailed {
                step: Step::Tcp,
                error: CoreError::Network {
                    kind: NetworkKind::Refused,
                    ..
                }
            }
        )));
        assert!(matches!(events.last(), Some(ConnectEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn unresolvable_host_is_dns_error() {
        let (result, _) = run(request("nonexistent.invalid", 22), CancellationToken::new()).await;
        assert!(matches!(
            result,
            Err(CoreError::Network {
                kind: NetworkKind::Dns,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn cancelled_token_aborts_pipeline() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (result, events) = run(request("127.0.0.1", free_port()), cancel).await;
        assert!(matches!(result, Err(CoreError::Cancelled)));
        assert!(events.iter().any(|e| matches!(
            e,
            ConnectEvent::StepFailed {
                step: Step::Tcp,
                error: CoreError::Cancelled
            }
        )));
    }

    #[tokio::test]
    async fn test_connection_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let rec = recorder();
        let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
        let result = test_connection(
            request("127.0.0.1", free_port()),
            deps(dir.path()),
            broker,
            rec.emit.clone(),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(CoreError::Network {
                kind: NetworkKind::Refused,
                ..
            })
        ));
        let events = rec.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            ConnectEvent::Steps { steps } if steps == &vec![Step::Tcp, Step::HostKey, Step::Auth]
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core --lib connect::pipeline_tests`
Expected: compilation fails with `cannot find function 'connect_ssh'`, `cannot find function 'test_connection'`, `cannot find type 'SshSession'`, `cannot find type 'OutputSink'`, `cannot find type 'CancellationToken'`, `use of undeclared type 'NetworkKind'`.

- [ ] **Step 3: Implement SshSession and the pipeline**

Create `crates/core/src/connect/ssh_session.rs`:

```rust
//! Interactive SSH shell session (implements `Session`).

use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::task::JoinHandle;

use super::classify::map_russh_error;
use super::handler::ClientHandler;
use crate::error::Result;
use crate::session::{OutputSink, Session, SessionEvent};

/// A shell channel plus the connection(s) that carry it.
pub struct SshSession {
    writer: ChannelWriteHalf<Msg>,
    handle: client::Handle<ClientHandler>,
    /// Keeps the jump host connection (and thus the tunnel) alive.
    jump: Option<client::Handle<ClientHandler>>,
    reader: JoinHandle<()>,
}

impl SshSession {
    /// Wrap an opened shell channel; spawns the reader task.
    pub(crate) fn start(
        channel: Channel<Msg>,
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        sink: OutputSink,
    ) -> Self {
        let (read_half, writer) = channel.split();
        let reader = tokio::spawn(forward_output(read_half, sink));
        Self {
            writer,
            handle,
            jump,
            reader,
        }
    }
}

/// Forward channel messages to the sink until the channel closes.
/// Exactly one `Exit` is emitted at the end.
async fn forward_output(mut read_half: ChannelReadHalf, sink: OutputSink) {
    let mut code = None;
    while let Some(msg) = read_half.wait().await {
        match msg {
            ChannelMsg::Data { data } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExtendedData { data, .. } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    sink(SessionEvent::Exit { code });
}

#[async_trait::async_trait]
impl Session for SshSession {
    async fn write(&self, data: &[u8]) -> Result<()> {
        self.writer.data(data).await.map_err(map_russh_error)
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.writer
            .window_change(u32::from(cols), u32::from(rows), 0, 0)
            .await
            .map_err(map_russh_error)
    }

    async fn close(&self) -> Result<()> {
        // Best effort: the remote side may already be gone.
        let _ = self.writer.close().await;
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
        if let Some(jump) = &self.jump {
            let _ = jump.disconnect(Disconnect::ByApplication, "", "en").await;
        }
        Ok(())
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.reader.abort();
    }
}
```

In `crates/core/src/connect/mod.rs`:

1. Change the module declarations at the top to:

```rust
pub mod auth;
pub mod classify;
pub mod handler;
pub mod prompt;
pub mod ssh_session;
```

2. Replace the whole `use`/`pub use` block (from `use std::path::PathBuf;` down to `use crate::session::SessionId;`) with:

```rust
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

pub use classify::{
    classify_io, classify_resolve_error, map_russh_error, network_error, sanitize_log_line,
};
pub use handler::{decide_host_key, ClientHandler, HandlerError, HostKeyAction};
pub use prompt::{EmitFn, HostKeyDecision, KbdPrompt, Prompt, PromptAnswer, PromptBroker};
pub use ssh_session::SshSession;

use crate::error::{CoreError, NetworkKind, Result};
use crate::known_hosts::KnownHosts;
use crate::model::{Host, LoginProfile};
use crate::secrets::SecretStore;
use crate::session::{OutputSink, SessionId};
```

3. Insert directly after the `plan_steps` function (before `#[cfg(test)] mod tests`):

```rust
/// Byte stream an SSH handshake can run over (TCP or a jump tunnel).
trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}

const TERM: &str = "xterm-256color";

/// Shared pipeline context for one connect attempt.
struct Pipeline {
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<dyn SecretStore>,
    agent_sock: Option<PathBuf>,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    cancel: CancellationToken,
}

/// An authenticated connection to the target (plus jump connection).
struct Established {
    handle: client::Handle<ClientHandler>,
    jump: Option<client::Handle<ClientHandler>>,
}

impl Pipeline {
    fn new(
        deps: ConnectDeps,
        broker: Arc<PromptBroker>,
        emit: EmitFn,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            known_hosts: Arc::new(deps.known_hosts),
            secrets: deps.secrets,
            agent_sock: deps.agent_sock,
            broker,
            emit,
            cancel,
        }
    }

    fn log(&self, line: impl Into<String>) {
        (self.emit)(log_line(line));
    }

    /// Run one step: emits Started/Done/Failed and aborts on cancellation.
    async fn step<T, F>(&self, step: Step, fut: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        (self.emit)(ConnectEvent::StepStarted { step });
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(CoreError::Cancelled),
            r = fut => r,
        };
        match &result {
            Ok(_) => (self.emit)(ConnectEvent::StepDone { step }),
            Err(error) => (self.emit)(ConnectEvent::StepFailed {
                step,
                error: error.clone(),
            }),
        }
        result
    }

    /// Resolve and connect with the host's timeout (0 = no timeout).
    async fn tcp_connect(&self, host: &Host) -> Result<TcpStream> {
        let address = host.address.clone();
        let port = host.port;
        let connect = async {
            self.log(format!("debug1: Connecting to {address} port {port}."));
            let addrs: Vec<_> = tokio::net::lookup_host((address.as_str(), port))
                .await
                .map_err(|e| {
                    network_error(
                        classify_resolve_error(&e),
                        format!("Could not resolve hostname {address}: {e}"),
                    )
                })?
                .collect();
            if addrs.is_empty() {
                return Err(network_error(
                    NetworkKind::Dns,
                    format!("Could not resolve hostname {address}: no addresses"),
                ));
            }
            let mut last_error = None;
            for addr in addrs {
                match TcpStream::connect(addr).await {
                    Ok(stream) => {
                        let _ = stream.set_nodelay(true);
                        self.log(format!("debug1: Connection established to {addr}."));
                        return Ok(stream);
                    }
                    Err(e) => {
                        self.log(format!("debug1: connect to {addr} failed: {e}"));
                        last_error = Some(e);
                    }
                }
            }
            let e = last_error.expect("at least one address was tried");
            Err(network_error(
                classify_io(&e),
                format!("connect to host {address} port {port}: {e}"),
            ))
        };
        if host.timeout_s == 0 {
            return connect.await;
        }
        match tokio::time::timeout(Duration::from_secs(u64::from(host.timeout_s)), connect).await {
            Ok(result) => result,
            Err(_) => Err(network_error(
                NetworkKind::Timeout,
                format!(
                    "connect to host {address} port {port}: Connection timed out after {}s",
                    host.timeout_s
                ),
            )),
        }
    }

    fn client_config(host: &Host) -> Arc<client::Config> {
        Arc::new(client::Config {
            keepalive_interval: (host.keepalive_s > 0)
                .then(|| Duration::from_secs(u64::from(host.keepalive_s))),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        })
    }

    /// SSH handshake incl. host key verification over `stream`.
    async fn handshake(
        &self,
        host: &Host,
        stream: Box<dyn Transport>,
    ) -> Result<client::Handle<ClientHandler>> {
        let handler = ClientHandler::new(
            host.address.clone(),
            host.port,
            self.known_hosts.clone(),
            self.broker.clone(),
            self.emit.clone(),
            self.cancel.clone(),
        );
        client::connect_stream(Self::client_config(host), stream, handler)
            .await
            .map_err(CoreError::from)
    }

    async fn authenticate(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        target: &ConnectTarget,
    ) -> Result<()> {
        let user = auth::effective_user(target)?;
        let (auth_config, owner) = auth::resolve_auth(target)?;
        let ctx = auth::AuthContext {
            user,
            host: target.host.address.clone(),
            auth: auth_config,
            owner,
            secrets: self.secrets.clone(),
            agent_sock: self.agent_sock.as_deref(),
            broker: &self.broker,
            emit: &self.emit,
            cancel: &self.cancel,
        };
        let mut authenticator = auth::HandleAuthenticator { handle };
        auth::authenticate(&mut authenticator, &ctx).await
    }

    /// Full connection to the jump host (own host key check and auth).
    async fn connect_jump(&self, jump: &ConnectTarget) -> Result<client::Handle<ClientHandler>> {
        self.log(format!(
            "debug1: Connecting to jump host {}",
            jump.host.address
        ));
        let stream = self.tcp_connect(&jump.host).await?;
        let mut handle = self.handshake(&jump.host, Box::new(stream)).await?;
        self.authenticate(&mut handle, jump).await?;
        Ok(handle)
    }

    /// Steps JumpHost? -> Tcp -> HostKey -> Auth.
    async fn establish(&self, req: &ConnectRequest) -> Result<Established> {
        let jump = match &req.jump {
            Some(jump_target) => Some(
                self.step(Step::JumpHost, self.connect_jump(jump_target))
                    .await?,
            ),
            None => None,
        };
        let host = &req.target.host;
        let stream: Box<dyn Transport> = self
            .step(Step::Tcp, async {
                match &jump {
                    Some(jump_handle) => {
                        self.log(format!(
                            "debug1: Opening direct-tcpip channel to {}:{} via jump host",
                            host.address, host.port
                        ));
                        let channel = jump_handle
                            .channel_open_direct_tcpip(
                                host.address.clone(),
                                u32::from(host.port),
                                "127.0.0.1",
                                0,
                            )
                            .await
                            .map_err(|e| {
                                network_error(
                                    NetworkKind::Other,
                                    format!(
                                        "jump host could not connect to {}:{}: {e}",
                                        host.address, host.port
                                    ),
                                )
                            })?;
                        Ok(Box::new(channel.into_stream()) as Box<dyn Transport>)
                    }
                    None => Ok(Box::new(self.tcp_connect(host).await?) as Box<dyn Transport>),
                }
            })
            .await?;
        let mut handle = self
            .step(Step::HostKey, self.handshake(host, stream))
            .await?;
        self.step(Step::Auth, self.authenticate(&mut handle, &req.target))
            .await?;
        Ok(Established { handle, jump })
    }

    /// Steps Channel -> Shell; returns the running session.
    async fn open_shell(
        &self,
        req: &ConnectRequest,
        est: Established,
        sink: OutputSink,
    ) -> Result<SshSession> {
        let host = &req.target.host;
        let channel = self
            .step(Step::Channel, async {
                self.log("debug1: Requesting session channel");
                let channel = est
                    .handle
                    .channel_open_session()
                    .await
                    .map_err(map_russh_error)?;
                self.log(format!(
                    "debug1: Requesting PTY {TERM} {}x{}",
                    req.cols, req.rows
                ));
                channel
                    .request_pty(
                        false,
                        TERM,
                        u32::from(req.cols),
                        u32::from(req.rows),
                        0,
                        0,
                        &[],
                    )
                    .await
                    .map_err(map_russh_error)?;
                for var in &host.env {
                    self.log(format!("debug1: Sending env {}", var.key));
                    channel
                        .set_env(false, var.key.clone(), var.value.clone())
                        .await
                        .map_err(map_russh_error)?;
                }
                Ok(channel)
            })
            .await?;
        self.step(Step::Shell, async {
            self.log("debug1: Requesting shell");
            channel
                .request_shell(false)
                .await
                .map_err(map_russh_error)?;
            if let Some(command) = host
                .start_command
                .as_deref()
                .filter(|c| !c.trim().is_empty())
            {
                self.log("debug1: Sending start command");
                channel
                    .data(format!("{command}\n").as_bytes())
                    .await
                    .map_err(map_russh_error)?;
            }
            Ok(())
        })
        .await?;
        Ok(SshSession::start(channel, est.handle, est.jump, sink))
    }
}

async fn disconnect(est: Established) {
    let _ = est
        .handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;
    if let Some(jump) = est.jump {
        let _ = jump
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

/// Run the full pipeline and return the interactive session.
/// The caller registers the session and emits `ConnectEvent::Connected`.
pub async fn connect_ssh(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    sink: OutputSink,
    cancel: CancellationToken,
) -> Result<SshSession> {
    emit(ConnectEvent::Steps {
        steps: plan_steps(&req),
    });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    let result = async {
        let est = pipeline.establish(&req).await?;
        pipeline.open_shell(&req, est, sink).await
    }
    .await;
    if let Err(error) = &result {
        emit(ConnectEvent::Failed {
            error: error.clone(),
        });
    }
    result
}

/// "Verbindung testen": run until authentication succeeded, then disconnect.
pub async fn test_connection(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    cancel: CancellationToken,
) -> Result<()> {
    let steps = plan_steps(&req)
        .into_iter()
        .filter(|s| !matches!(s, Step::Channel | Step::Shell))
        .collect();
    emit(ConnectEvent::Steps { steps });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    match pipeline.establish(&req).await {
        Ok(est) => {
            disconnect(est).await;
            Ok(())
        }
        Err(error) => {
            emit(ConnectEvent::Failed {
                error: error.clone(),
            });
            Err(error)
        }
    }
}
```

Design notes: every step future runs inside `Pipeline::step`, whose `select!` (biased towards cancellation) drops the in-flight future on `cancel`, which closes the TCP stream / russh handles. The host key prompt (inside `check_server_key`) and auth prompts use `ask_cancellable` with the same token, so cancellation also unblocks a waiting prompt. Keepalive comes from `host.keepalive_s` via `client::Config::keepalive_interval` (0 disables), `keepalive_max = 3`. The jump hop has its own `ClientHandler` (own known_hosts check for the jump address/port) and its own auth; the target handshake runs over `channel_open_direct_tcpip(..).into_stream()` with `client::connect_stream`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p linascp-core --lib connect`
Expected: `test result: ok. 40 passed; 0 failed` (36 earlier + 4 pipeline tests)

Then run the whole crate: `cargo test -p linascp-core`
Expected: all tests pass, no warnings about unused imports in `connect`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/connect/mod.rs crates/core/src/connect/ssh_session.rs
git commit -m "feat(core): add SSH connect pipeline with jump host and SshSession

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Docker-based SSH integration tests

**Files:**
- Create: `crates/core/tests/fixtures/sshd/Dockerfile`
- Create: `crates/core/tests/fixtures/sshd/sshd_config`
- Create: `crates/core/tests/support/mod.rs`
- Create: `crates/core/tests/ssh_integration.rs`

**Interfaces:**
- Consumes: public API of `linascp_core` (`connect::{connect_ssh, test_connection, ConnectDeps, ConnectRequest, ConnectTarget, ConnectEvent, EmitFn, HostKeyDecision, Prompt, PromptAnswer, PromptBroker, SshSession}`, `error::{CoreError, NetworkKind, Result}`, `known_hosts::{HostKeyStatus, KnownHosts}` (`HostKeyStatus: Debug + PartialEq`), `model::{AuthConfig, AuthMethod, EnvVar, Host, HostAuth}`, `secrets::{MemorySecretStore, SecretKind, SecretOwner, SecretStore}`, `session::{OutputSink, Session, SessionEvent}`); host tools `docker`, `ssh-keygen`, `ssh-agent`, `ssh-add`.
- Produces: test binary `ssh_integration` (all tests `#[ignore = "requires docker"]`), run with `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`.

Fixture design: one image `linascp-test-sshd:latest` (Debian trixie + OpenSSH 10). Users: `pwuser`/`pwpass` (password), `keyuser` (authorized keys baked into the image from keys generated per test run), `kbduser`/`kbdpass` restricted to `AuthenticationMethods keyboard-interactive`. Keyboard-interactive needs PAM on Linux (OpenSSH has no other kbd-interactive device there), so the config keeps `UsePAM yes`; `pam_loginuid` is made optional because it fails inside containers. Host keys are generated by the Debian package at build time, so every container of the image shares them (used by the jump test). Ports are published to random `127.0.0.1` ports; the jump test puts both containers on a temporary docker network and reaches the target by container name.

- [ ] **Step 1: Create the sshd fixture**

Create `crates/core/tests/fixtures/sshd/Dockerfile`:

```dockerfile
FROM debian:trixie-slim
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd /etc/ssh/authorized_keys \
 && sed -i 's/^session\s\+required\s\+pam_loginuid.so/session optional pam_loginuid.so/' /etc/pam.d/sshd \
 && useradd -m -s /bin/bash pwuser && echo 'pwuser:pwpass' | chpasswd \
 && useradd -m -s /bin/bash keyuser && echo 'keyuser:unused-Kq81' | chpasswd \
 && useradd -m -s /bin/bash kbduser && echo 'kbduser:kbdpass' | chpasswd
COPY sshd_config /etc/ssh/sshd_config
COPY keyuser.authorized_keys /etc/ssh/authorized_keys/keyuser
RUN chmod 644 /etc/ssh/authorized_keys/keyuser && sshd -t
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
```

Create `crates/core/tests/fixtures/sshd/sshd_config`:

```
# Test-only sshd configuration for LinaSCP integration tests.
Port 22
HostKey /etc/ssh/ssh_host_ed25519_key
HostKey /etc/ssh/ssh_host_ecdsa_key
HostKey /etc/ssh/ssh_host_rsa_key
UsePAM yes
PasswordAuthentication yes
KbdInteractiveAuthentication yes
PubkeyAuthentication yes
AuthorizedKeysFile /etc/ssh/authorized_keys/%u
AllowTcpForwarding yes
AcceptEnv LANG LC_* LINASCP_*
PrintMotd no
Subsystem sftp /usr/lib/openssh/sftp-server

Match User kbduser
    AuthenticationMethods keyboard-interactive
```

(`keyuser.authorized_keys` is not committed; the harness writes it into a temporary build context.)

- [ ] **Step 2: Create the test harness**

Create `crates/core/tests/support/mod.rs`:

```rust
//! Docker sshd harness for the SSH integration tests.
#![allow(dead_code)]

use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use linascp_core::connect::{ConnectEvent, EmitFn, Prompt, PromptAnswer, PromptBroker};
use linascp_core::model::{AuthConfig, AuthMethod, Host, HostAuth};
use linascp_core::session::{OutputSink, SessionEvent};

pub const IMAGE: &str = "linascp-test-sshd:latest";
pub const KEY_PASSPHRASE: &str = "keypass";
const DOCKERFILE: &str = include_str!("../fixtures/sshd/Dockerfile");
const SSHD_CONFIG: &str = include_str!("../fixtures/sshd/sshd_config");

/// Client keys generated once per test run; the image authorizes all three.
pub struct Fixture {
    _dir: tempfile::TempDir,
    pub plain_key: PathBuf,
    pub encrypted_key: PathBuf,
    pub agent_key: PathBuf,
}

fn run(cmd: &mut Command) -> String {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("cannot run {cmd:?}: {e}"));
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn keygen(path: &Path, passphrase: &str) {
    run(Command::new("ssh-keygen")
        .args([
            "-q",
            "-t",
            "ed25519",
            "-a",
            "4",
            "-C",
            "linascp-test",
            "-N",
            passphrase,
            "-f",
        ])
        .arg(path));
}

/// Build the image (once per process) and return the client keys.
pub fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        let plain_key = dir.path().join("id_plain");
        let encrypted_key = dir.path().join("id_encrypted");
        let agent_key = dir.path().join("id_agent");
        keygen(&plain_key, "");
        keygen(&encrypted_key, KEY_PASSPHRASE);
        keygen(&agent_key, "");

        let ctx = dir.path().join("docker");
        std::fs::create_dir(&ctx).unwrap();
        std::fs::write(ctx.join("Dockerfile"), DOCKERFILE).unwrap();
        std::fs::write(ctx.join("sshd_config"), SSHD_CONFIG).unwrap();
        let mut authorized = String::new();
        for key in [&plain_key, &encrypted_key, &agent_key] {
            authorized.push_str(&std::fs::read_to_string(key.with_extension("pub")).unwrap());
        }
        std::fs::write(ctx.join("keyuser.authorized_keys"), authorized).unwrap();
        run(Command::new("docker")
            .args(["build", "-q", "-t", IMAGE])
            .arg(&ctx));

        Fixture {
            _dir: dir,
            plain_key,
            encrypted_key,
            agent_key,
        }
    })
}

/// A user-defined docker network, removed on drop.
pub struct Network {
    pub name: String,
}

impl Network {
    pub fn create() -> Self {
        let name = format!("linascp-test-{}", uuid::Uuid::new_v4().simple());
        run(Command::new("docker").args(["network", "create", &name]));
        Self { name }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["network", "rm", &self.name])
            .output();
    }
}

/// A running sshd container with port 22 published on 127.0.0.1.
pub struct Container {
    pub id: String,
    pub name: String,
    pub port: u16,
}

impl Container {
    pub fn start(network: Option<&Network>) -> Self {
        fixture();
        let name = format!("linascp-sshd-{}", uuid::Uuid::new_v4().simple());
        let mut cmd = Command::new("docker");
        cmd.args(["run", "-d", "--rm", "-p", "127.0.0.1::22", "--name", &name]);
        if let Some(net) = network {
            cmd.args(["--network", &net.name]);
        }
        cmd.arg(IMAGE);
        let id = run(&mut cmd);
        let mapping = run(Command::new("docker").args(["port", &id, "22/tcp"]));
        let port = mapping
            .lines()
            .next()
            .and_then(|l| l.rsplit(':').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("cannot parse port mapping {mapping:?}"));
        let container = Self { id, name, port };
        container.wait_ready();
        container
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4];
                if stream.read_exact(&mut buf).is_ok() && &buf == b"SSH-" {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("sshd in container {} did not become ready", self.name);
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.id]).output();
    }
}

/// `ssh-agent -D` on a temp socket holding the agent key; killed on drop.
pub struct Agent {
    child: Child,
    _dir: tempfile::TempDir,
    pub socket: PathBuf,
}

impl Agent {
    pub fn start(key: &Path) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("agent.sock");
        let child = Command::new("ssh-agent")
            .arg("-D")
            .arg("-a")
            .arg(&socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ssh-agent");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !socket.exists() {
            assert!(
                Instant::now() < deadline,
                "ssh-agent socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        run(Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", &socket)
            .arg(key)
            .stderr(Stdio::null()));
        Self {
            child,
            _dir: dir,
            socket,
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Answers prompts with `policy` and records all events.
pub struct Prompter {
    pub broker: Arc<PromptBroker>,
    pub emit: EmitFn,
    pub prompts: Arc<Mutex<Vec<Prompt>>>,
    pub events: Arc<Mutex<Vec<ConnectEvent>>>,
}

pub fn prompter(policy: impl Fn(&Prompt) -> PromptAnswer + Send + Sync + 'static) -> Prompter {
    let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (b, p, e) = (broker.clone(), prompts.clone(), events.clone());
    let emit: EmitFn = Arc::new(move |event| {
        if let ConnectEvent::Prompt { prompt_id, prompt } = &event {
            p.lock().unwrap().push(prompt.clone());
            b.answer(*prompt_id, policy(prompt));
        }
        e.lock().unwrap().push(event);
    });
    Prompter {
        broker,
        emit,
        prompts,
        events,
    }
}

impl Prompter {
    pub fn prompts(&self) -> Vec<Prompt> {
        self.prompts.lock().unwrap().clone()
    }
}

/// Collects session output and the exit event.
#[derive(Clone)]
pub struct Output {
    pub bytes: Arc<Mutex<Vec<u8>>>,
    pub exited: Arc<AtomicBool>,
}

impl Output {
    pub fn new() -> (Self, OutputSink) {
        let out = Self {
            bytes: Arc::new(Mutex::new(Vec::new())),
            exited: Arc::new(AtomicBool::new(false)),
        };
        let o = out.clone();
        let sink: OutputSink = Arc::new(move |event| match event {
            SessionEvent::Output(data) => o.bytes.lock().unwrap().extend_from_slice(&data),
            SessionEvent::Exit { .. } => o.exited.store(true, Ordering::SeqCst),
            SessionEvent::Error(_) => {}
        });
        (out, sink)
    }

    pub async fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if String::from_utf8_lossy(&self.bytes.lock().unwrap()).contains(needle) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "output never contained {needle:?}: {}",
                String::from_utf8_lossy(&self.bytes.lock().unwrap())
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn wait_exit(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.exited.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "session did not exit");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Host on 127.0.0.1:`port` with a custom auth method.
pub fn host(port: u16, user: &str, method: AuthMethod, key_path: Option<&Path>) -> Host {
    let mut host = Host::new("test", "127.0.0.1");
    host.port = port;
    host.user = Some(user.to_string());
    host.timeout_s = 10;
    host.auth = HostAuth::Custom {
        auth: AuthConfig {
            method,
            key_path: key_path.map(|p| p.display().to_string()),
            has_secret: false,
        },
    };
    host
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
```

- [ ] **Step 3: Write the integration tests**

Create `crates/core/tests/ssh_integration.rs`:

```rust
//! SSH pipeline against real OpenSSH servers in Docker.
//! Run: cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1
//! Needs: docker, ssh-keygen, ssh-agent, ssh-add.

mod support;

use std::sync::Arc;

use linascp_core::connect::{
    connect_ssh, test_connection, ConnectDeps, ConnectRequest, ConnectTarget, HostKeyDecision,
    Prompt, PromptAnswer, SshSession,
};
use linascp_core::error::{CoreError, NetworkKind, Result};
use linascp_core::known_hosts::{HostKeyStatus, KnownHosts};
use linascp_core::model::{AuthMethod, EnvVar, Host};
use linascp_core::secrets::{MemorySecretStore, SecretKind, SecretOwner, SecretStore};
use linascp_core::session::Session;
use secrecy::{ExposeSecret, SecretString};
use support::{
    free_port, host, prompter, Agent, Container, Network, Output, Prompter, KEY_PASSPHRASE,
};
use tokio_util::sync::CancellationToken;

/// Per-test environment: temp known_hosts + in-memory secrets.
struct Env {
    dir: tempfile::TempDir,
    secrets: Arc<MemorySecretStore>,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            secrets: Arc::new(MemorySecretStore::new()),
        }
    }

    fn known_hosts_path(&self) -> std::path::PathBuf {
        self.dir.path().join("known_hosts")
    }

    fn deps(&self, agent_sock: Option<std::path::PathBuf>) -> ConnectDeps {
        ConnectDeps {
            known_hosts: KnownHosts::new(self.known_hosts_path()),
            secrets: self.secrets.clone(),
            agent_sock,
        }
    }

    fn store_password(&self, host: &Host, password: &str) {
        self.secrets
            .set(
                SecretOwner::Host(host.id),
                SecretKind::Password,
                &SecretString::from(password.to_string()),
            )
            .unwrap();
    }
}

fn request(target: Host, jump: Option<Host>) -> ConnectRequest {
    ConnectRequest {
        target: ConnectTarget {
            host: target,
            profile: None,
        },
        jump: jump.map(|host| ConnectTarget {
            host,
            profile: None,
        }),
        cols: 80,
        rows: 24,
    }
}

/// Trust unknown host keys; fail on any other prompt.
fn trust_all(prompt: &Prompt) -> PromptAnswer {
    match prompt {
        Prompt::HostKeyUnknown { .. } => PromptAnswer::HostKey {
            decision: HostKeyDecision::Trust,
        },
        other => panic!("unexpected prompt {other:?}"),
    }
}

async fn connect(
    env: &Env,
    req: ConnectRequest,
    agent_sock: Option<std::path::PathBuf>,
    p: &Prompter,
) -> (Result<SshSession>, Output) {
    let (output, sink) = Output::new();
    let result = connect_ssh(
        req,
        env.deps(agent_sock),
        p.broker.clone(),
        p.emit.clone(),
        sink,
        CancellationToken::new(),
    )
    .await;
    (result, output)
}

/// Prove the shell works, then close and expect Exit.
async fn assert_shell(session: SshSession, output: &Output) {
    session.write(b"echo linascp-$((6*7))\n").await.unwrap();
    output.wait_for("linascp-42").await;
    session.resize(100, 30).await.unwrap();
    session.close().await.unwrap();
    output.wait_exit().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn password_auth_with_env_and_start_command() {
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.env = vec![EnvVar {
        key: "LINASCP_TEST".into(),
        value: "envok".into(),
    }];
    h.start_command = Some("echo start-$((1+1)) $LINASCP_TEST".into());
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);

    let (session, output) = connect(&env, request(h, None), None, &p).await;
    let session = session.expect("connected");
    output.wait_for("start-2 envok").await;
    assert_shell(session, &output).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn key_auth_without_passphrase() {
    let c = Container::start(None);
    let env = Env::new();
    let fx = support::fixture();
    let h = host(c.port, "keyuser", AuthMethod::Key, Some(&fx.plain_key));
    let p = prompter(trust_all);
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn key_auth_with_passphrase_prompt_and_save() {
    let c = Container::start(None);
    let env = Env::new();
    let fx = support::fixture();
    let h = host(c.port, "keyuser", AuthMethod::Key, Some(&fx.encrypted_key));
    let owner = SecretOwner::Host(h.id);
    let p = prompter(|prompt| match prompt {
        Prompt::Passphrase { .. } => PromptAnswer::Secret {
            value: KEY_PASSPHRASE.into(),
            save: true,
        },
        other => trust_all(other),
    });
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
    let saved = env
        .secrets
        .get(owner, SecretKind::Passphrase)
        .unwrap()
        .unwrap();
    assert_eq!(saved.expose_secret(), KEY_PASSPHRASE);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn agent_auth() {
    let c = Container::start(None);
    let env = Env::new();
    let agent = Agent::start(&support::fixture().agent_key);
    let h = host(c.port, "keyuser", AuthMethod::Agent, None);
    let p = prompter(trust_all);
    let (session, output) = connect(&env, request(h, None), Some(agent.socket.clone()), &p).await;
    assert_shell(session.expect("connected"), &output).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn keyboard_interactive_auth() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "kbduser", AuthMethod::KeyboardInteractive, None);
    let p = prompter(|prompt| match prompt {
        Prompt::KeyboardInteractive { prompts, .. } => PromptAnswer::Responses {
            values: prompts.iter().map(|_| "kbdpass".to_string()).collect(),
        },
        other => trust_all(other),
    });
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
    assert!(p.prompts().iter().any(
        |pr| matches!(pr, Prompt::KeyboardInteractive { prompts, .. } if !prompts.is_empty())
    ));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn unknown_host_key_trust_writes_known_hosts() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");

    let p = prompter(trust_all);
    let (session, output) = connect(&env, request(h.clone(), None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
    assert!(matches!(p.prompts()[0], Prompt::HostKeyUnknown { .. }));
    assert!(env.known_hosts_path().exists());

    // Second connect: key is known, no host key prompt allowed.
    let p2 = prompter(|other| panic!("unexpected prompt {other:?}"));
    let (session, output) = connect(&env, request(h, None), None, &p2).await;
    assert_shell(session.expect("connected again"), &output).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn changed_host_key_reject_fails_closed() {
    let c = Container::start(None);
    let env = Env::new();
    // Pretend the server used to have a different ed25519 key.
    let fake =
        russh::keys::load_public_key(support::fixture().plain_key.with_extension("pub")).unwrap();
    let kh = KnownHosts::new(env.known_hosts_path());
    kh.add("127.0.0.1", c.port, &fake, false).unwrap();
    assert_eq!(
        kh.check("127.0.0.1", c.port, &fake).unwrap(),
        HostKeyStatus::Known
    );

    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(|prompt| match prompt {
        Prompt::HostKeyChanged { .. } => PromptAnswer::HostKey {
            decision: HostKeyDecision::Reject,
        },
        other => panic!("unexpected prompt {other:?}"),
    });
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    assert!(matches!(result, Err(CoreError::HostKeyRejected { .. })));
    assert!(matches!(p.prompts()[0], Prompt::HostKeyChanged { .. }));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn jump_host_tunnel() {
    let net = Network::create();
    let target = Container::start(Some(&net));
    let jump = Container::start(Some(&net));
    let env = Env::new();

    let jump_host = host(jump.port, "pwuser", AuthMethod::Password, None);
    let mut target_host = host(22, "pwuser", AuthMethod::Password, None);
    // Resolved by the jump host inside the docker network.
    target_host.address = target.name.clone();
    env.store_password(&jump_host, "pwpass");
    env.store_password(&target_host, "pwpass");

    let p = prompter(trust_all);
    let (session, output) = connect(&env, request(target_host, Some(jump_host)), None, &p).await;
    assert_shell(session.expect("connected via jump"), &output).await;
    let host_key_prompts = p
        .prompts()
        .iter()
        .filter(|pr| matches!(pr, Prompt::HostKeyUnknown { .. }))
        .count();
    assert_eq!(
        host_key_prompts, 2,
        "jump and target keys are checked separately"
    );
}

#[tokio::test]
#[ignore = "requires docker"]
async fn refused_port_is_network_refused() {
    let env = Env::new();
    let h = host(free_port(), "pwuser", AuthMethod::Password, None);
    let p = prompter(trust_all);
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    assert!(matches!(
        result,
        Err(CoreError::Network {
            kind: NetworkKind::Refused,
            ..
        })
    ));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn dns_failure_is_network_dns() {
    let env = Env::new();
    let mut h = host(22, "pwuser", AuthMethod::Password, None);
    h.address = "nonexistent.invalid".into();
    let p = prompter(trust_all);
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    assert!(matches!(
        result,
        Err(CoreError::Network {
            kind: NetworkKind::Dns,
            ..
        })
    ));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn wrong_password_reports_rejected_methods() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    let p = prompter(|prompt| match prompt {
        Prompt::Password { .. } => PromptAnswer::Secret {
            value: "wrong".into(),
            save: true,
        },
        other => trust_all(other),
    });
    let (result, _) = connect(&env, request(h.clone(), None), None, &p).await;
    match result {
        Err(CoreError::Auth {
            rejected_methods, ..
        }) => assert!(rejected_methods.iter().any(|m| m == "password")),
        Err(other) => panic!("unexpected error {other:?}"),
        Ok(_) => panic!("wrong password must not authenticate"),
    }
    let password_prompts = p
        .prompts()
        .iter()
        .filter(|pr| matches!(pr, Prompt::Password { .. }))
        .count();
    assert_eq!(password_prompts, 3);
    assert!(env
        .secrets
        .get(SecretOwner::Host(h.id), SecretKind::Password)
        .unwrap()
        .is_none());
}

#[tokio::test]
#[ignore = "requires docker"]
async fn test_connection_stops_after_auth() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);
    test_connection(
        request(h, None),
        env.deps(None),
        p.broker.clone(),
        p.emit.clone(),
        CancellationToken::new(),
    )
    .await
    .expect("test connection succeeds");
}
```

- [ ] **Step 4: Verify the tests compile and are skipped by default**

Run: `cargo test -p linascp-core --test ssh_integration`
Expected: `test result: ok. 0 passed; 0 failed; 12 ignored`

- [ ] **Step 5: Run the Docker integration tests**

Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
Expected: `test result: ok. 12 passed; 0 failed` (first run also builds the image, ~1–2 min; afterwards ~30–60 s). Afterwards `docker ps -a --filter name=linascp` and `docker network ls --filter name=linascp-test` show nothing left over.

If a test fails, inspect the container log while it runs with `docker logs <name>` (sshd runs with `-e`, logging to stderr).

- [ ] **Step 6: Run the full crate test suite without Docker**

Run: `cargo test -p linascp-core`
Expected: all unit tests pass; the 12 integration tests are reported as ignored.

- [ ] **Step 7: Commit**

```bash
git add crates/core/tests/fixtures/sshd/Dockerfile crates/core/tests/fixtures/sshd/sshd_config crates/core/tests/support/mod.rs crates/core/tests/ssh_integration.rs
git commit -m "test(core): add Docker sshd integration tests for the SSH pipeline

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
