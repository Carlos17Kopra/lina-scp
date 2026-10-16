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
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {cmd:?}: {e}"));
    assert!(out.status.success(), "{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn keygen(path: &Path, passphrase: &str) {
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-a", "4", "-C", "linascp-test", "-N", passphrase, "-f"])
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
        run(Command::new("docker").args(["build", "-q", "-t", IMAGE]).arg(&ctx));

        Fixture { _dir: dir, plain_key, encrypted_key, agent_key }
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
        let _ = Command::new("docker").args(["network", "rm", &self.name]).output();
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
        // Construct the guard right after the container exists, so a panic in the
        // fallible steps below still runs `Drop` (`docker rm -f`) instead of leaking it.
        let mut container = Self { id, name, port: 0 };
        let mapping = run(Command::new("docker").args(["port", &container.id, "22/tcp"]));
        container.port = mapping
            .lines()
            .next()
            .and_then(|l| l.rsplit(':').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("cannot parse port mapping {mapping:?}"));
        container.wait_ready();
        container
    }

    /// The server's public host key of `kind` (`ed25519`, `ecdsa`, `rsa`) as `<algorithm> <base64>`.
    /// Runs a command in the container as root; returns its trimmed stdout.
    pub fn exec(&self, args: &[&str]) -> String {
        run(Command::new("docker").args(["exec", &self.id]).args(args))
    }

    pub fn host_public_key(&self, kind: &str) -> String {
        let line =
            run(Command::new("docker").args(["exec", &self.id, "cat", &format!("/etc/ssh/ssh_host_{kind}_key.pub")]));
        line.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
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
        // Construct the guard right after the child is spawned, so a panic in the
        // fallible steps below still runs `Drop` (kills the child) instead of orphaning it.
        let agent = Self { child, _dir: dir, socket };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !agent.socket.exists() {
            assert!(Instant::now() < deadline, "ssh-agent socket did not appear");
            std::thread::sleep(Duration::from_millis(50));
        }
        run(Command::new("ssh-add").env("SSH_AUTH_SOCK", &agent.socket).arg(key).stderr(Stdio::null()));
        agent
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
    Prompter { broker, emit, prompts, events }
}

impl Prompter {
    pub fn prompts(&self) -> Vec<Prompt> {
        self.prompts.lock().unwrap().clone()
    }

    pub fn events(&self) -> Vec<ConnectEvent> {
        self.events.lock().unwrap().clone()
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
        let out = Self { bytes: Arc::new(Mutex::new(Vec::new())), exited: Arc::new(AtomicBool::new(false)) };
        let o = out.clone();
        let sink: OutputSink = Arc::new(move |event| match event {
            SessionEvent::Output(data) => o.bytes.lock().unwrap().extend_from_slice(&data),
            SessionEvent::Exit { .. } => o.exited.store(true, Ordering::SeqCst),
            _ => {}
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
        auth: AuthConfig { method, key_path: key_path.map(|p| p.display().to_string()), has_secret: false },
    };
    host
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
