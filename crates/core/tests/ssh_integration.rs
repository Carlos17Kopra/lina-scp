//! SSH pipeline against real OpenSSH servers in Docker.
//! Run: cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1
//! Needs: docker, ssh-keygen, ssh-agent, ssh-add.

mod support;

use std::sync::Arc;

use linascp_core::connect::{
    ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, HostKeyDecision, LostFn, Prompt, PromptAnswer,
    SshSession, Step, connect_sftp, connect_ssh, test_connection,
};
use linascp_core::error::{CoreError, FsKind, NetworkKind, Result};
use linascp_core::known_hosts::{HostKeyStatus, KnownHosts};
use linascp_core::model::{AuthMethod, EnvVar, Host};
use linascp_core::secrets::{MemorySecretStore, SecretKind, SecretOwner, SecretStore};
use linascp_core::session::Session;
use linascp_core::sftp::{EntryKind, LinkTarget};
use secrecy::{ExposeSecret, SecretString};
use support::{Agent, Container, KEY_PASSPHRASE, Network, Output, Prompter, free_port, host, prompter};
use tokio_util::sync::CancellationToken;

/// Per-test environment: temp known_hosts + in-memory secrets.
struct Env {
    dir: tempfile::TempDir,
    secrets: Arc<MemorySecretStore>,
}

impl Env {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), secrets: Arc::new(MemorySecretStore::new()) }
    }

    fn known_hosts_path(&self) -> std::path::PathBuf {
        self.dir.path().join("known_hosts")
    }

    fn deps(&self, agent_sock: Option<std::path::PathBuf>) -> ConnectDeps {
        ConnectDeps { known_hosts: KnownHosts::new(self.known_hosts_path()), secrets: self.secrets.clone(), agent_sock }
    }

    fn store_password(&self, host: &Host, password: &str) {
        self.secrets
            .set(SecretOwner::Host(host.id), SecretKind::Password, &SecretString::from(password.to_string()))
            .unwrap();
    }
}

fn request(target: Host, jump: Option<Host>) -> ConnectRequest {
    ConnectRequest {
        target: ConnectTarget { host: target, profile: None },
        jump: jump.map(|host| ConnectTarget { host, profile: None }),
        cols: 80,
        rows: 24,
    }
}

/// Trust unknown host keys; fail on any other prompt.
fn trust_all(prompt: &Prompt) -> PromptAnswer {
    match prompt {
        Prompt::HostKeyUnknown { .. } => PromptAnswer::HostKey { decision: HostKeyDecision::Trust },
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
    let result =
        connect_ssh(req, env.deps(agent_sock), p.broker.clone(), p.emit.clone(), sink, CancellationToken::new()).await;
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
    h.env = vec![EnvVar { key: "LINASCP_TEST".into(), value: "envok".into() }];
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
        Prompt::Passphrase { .. } => PromptAnswer::Secret { value: KEY_PASSPHRASE.into(), save: true },
        other => trust_all(other),
    });
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
    let saved = env.secrets.get(owner, SecretKind::Passphrase).unwrap().unwrap();
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
        Prompt::KeyboardInteractive { prompts, .. } => {
            PromptAnswer::Responses { values: prompts.iter().map(|_| "kbdpass".to_string()).collect() }
        }
        other => trust_all(other),
    });
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected"), &output).await;
    assert!(
        p.prompts().iter().any(|pr| matches!(pr, Prompt::KeyboardInteractive { prompts, .. } if !prompts.is_empty()))
    );
}

/// F4: `AuthenticationMethods publickey,keyboard-interactive` — the key is accepted with partial
/// success, then the keyboard-interactive round (PAM password prompt) completes the login.
#[tokio::test]
#[ignore = "requires docker"]
async fn key_then_keyboard_interactive_two_factor() {
    let c = Container::start(None);
    let env = Env::new();
    let fx = support::fixture();
    let h = host(c.port, "mfauser", AuthMethod::Key, Some(&fx.plain_key));
    let p = prompter(|prompt| match prompt {
        Prompt::KeyboardInteractive { prompts, .. } => {
            PromptAnswer::Responses { values: prompts.iter().map(|_| "mfapass".to_string()).collect() }
        }
        other => trust_all(other),
    });
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    let session = session.expect("connected with key + keyboard-interactive");
    assert_eq!(session.info().auth_methods, vec![AuthMethod::Key, AuthMethod::KeyboardInteractive]);
    assert_shell(session, &output).await;
    assert!(p.prompts().iter().any(|pr| matches!(pr, Prompt::KeyboardInteractive { .. })));
    let events = p.events.lock().unwrap().clone();
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepDone { step: Step::Auth })));
}

/// F4: `AuthenticationMethods publickey,password` — the password round uses the stored password.
#[tokio::test]
#[ignore = "requires docker"]
async fn key_then_password_two_factor() {
    let c = Container::start(None);
    let env = Env::new();
    let fx = support::fixture();
    let h = host(c.port, "mfpwuser", AuthMethod::Key, Some(&fx.plain_key));
    env.store_password(&h, "mfpwpass");
    let p = prompter(trust_all);
    let (session, output) = connect(&env, request(h, None), None, &p).await;
    assert_shell(session.expect("connected with key + password"), &output).await;
}

/// F4: a wrong second factor after an accepted key -> an auth error naming the remaining method.
#[tokio::test]
#[ignore = "requires docker"]
async fn key_without_second_factor_reports_remaining_methods() {
    let c = Container::start(None);
    let env = Env::new();
    let fx = support::fixture();
    let h = host(c.port, "mfauser", AuthMethod::Key, Some(&fx.plain_key));
    let p = prompter(|prompt| match prompt {
        Prompt::KeyboardInteractive { .. } => PromptAnswer::Responses { values: vec!["wrong".into()] },
        other => trust_all(other),
    });
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    match result {
        Err(CoreError::Auth { rejected_methods, .. }) => {
            assert!(rejected_methods.iter().any(|m| m == "keyboard-interactive"), "{rejected_methods:?}")
        }
        Err(other) => panic!("unexpected error {other:?}"),
        Ok(_) => panic!("a wrong second factor must not authenticate"),
    }
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
    let session = session.expect("connected");
    let info = session.info().clone();
    let Prompt::HostKeyUnknown { info: shown, .. } = &p.prompts()[0] else { panic!("expected host key prompt") };
    assert_eq!(info.host_key_sha256, shown.sha256);
    assert_eq!(info.host_key_algorithm, shown.algorithm);
    assert!(!info.kex.is_empty() && !info.cipher.is_empty() && !info.mac.is_empty(), "{info:?}");
    assert_eq!(info.auth_methods, vec![AuthMethod::Password]);
    assert_shell(session, &output).await;
    assert!(env.known_hosts_path().exists());

    // Second connect: key is known, no host key prompt allowed.
    let p2 = prompter(|other| panic!("unexpected prompt {other:?}"));
    let (session, output) = connect(&env, request(h, None), None, &p2).await;
    assert_shell(session.expect("connected again"), &output).await;
}

/// F3: a host known (as in OpenSSH) only by a non-default key type negotiates that type and is
/// accepted without a host-key prompt.
#[tokio::test]
#[ignore = "requires docker"]
async fn host_known_only_by_ecdsa_or_rsa_connects_without_prompt() {
    let c = Container::start(None);
    for kind in ["ecdsa", "rsa"] {
        let env = Env::new();
        std::fs::write(env.known_hosts_path(), format!("[127.0.0.1]:{} {}\n", c.port, c.host_public_key(kind)))
            .unwrap();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let p = prompter(|other| panic!("unexpected prompt {other:?}"));
        let (session, output) = connect(&env, request(h, None), None, &p).await;
        assert_shell(session.unwrap_or_else(|e| panic!("{kind}: {e:?}")), &output).await;
        assert!(p.prompts().is_empty(), "{kind}: no prompt expected");
    }
}

#[tokio::test]
#[ignore = "requires docker"]
async fn changed_host_key_reject_fails_closed() {
    let c = Container::start(None);
    let env = Env::new();
    // Pretend the server used to have a different ed25519 key.
    let fake = russh::keys::load_public_key(support::fixture().plain_key.with_extension("pub")).unwrap();
    let kh = KnownHosts::new(env.known_hosts_path());
    kh.add("127.0.0.1", c.port, &fake, false).unwrap();
    assert_eq!(kh.check("127.0.0.1", c.port, &fake).unwrap(), HostKeyStatus::Known);

    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(|prompt| match prompt {
        Prompt::HostKeyChanged { .. } => PromptAnswer::HostKey { decision: HostKeyDecision::Reject },
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
    let host_key_prompts = p.prompts().iter().filter(|pr| matches!(pr, Prompt::HostKeyUnknown { .. })).count();
    assert_eq!(host_key_prompts, 2, "jump and target keys are checked separately");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn refused_port_is_network_refused() {
    let env = Env::new();
    let h = host(free_port(), "pwuser", AuthMethod::Password, None);
    let p = prompter(trust_all);
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Refused, .. })));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn dns_failure_is_network_dns() {
    let env = Env::new();
    let mut h = host(22, "pwuser", AuthMethod::Password, None);
    h.address = "nonexistent.invalid".into();
    let p = prompter(trust_all);
    let (result, _) = connect(&env, request(h, None), None, &p).await;
    assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Dns, .. })));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn wrong_password_reports_rejected_methods() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    let p = prompter(|prompt| match prompt {
        Prompt::Password { .. } => PromptAnswer::Secret { value: "wrong".into(), save: true },
        other => trust_all(other),
    });
    let (result, _) = connect(&env, request(h.clone(), None), None, &p).await;
    match result {
        Err(CoreError::Auth { rejected_methods, .. }) => assert!(rejected_methods.iter().any(|m| m == "password")),
        Err(other) => panic!("unexpected error {other:?}"),
        Ok(_) => panic!("wrong password must not authenticate"),
    }
    let password_prompts = p.prompts().iter().filter(|pr| matches!(pr, Prompt::Password { .. })).count();
    assert_eq!(password_prompts, 3);
    assert!(env.secrets.get(SecretOwner::Host(h.id), SecretKind::Password).unwrap().is_none());
}

#[tokio::test]
#[ignore = "requires docker"]
async fn test_connection_stops_after_auth() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);
    test_connection(request(h, None), env.deps(None), p.broker.clone(), p.emit.clone(), CancellationToken::new())
        .await
        .expect("test connection succeeds");

    // `test_connection` must stop after Auth: no Channel/Shell step is planned or started.
    let events = p.events.lock().unwrap().clone();
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::Steps { steps } if steps.contains(&Step::Auth))));
    assert!(!events.iter().any(|e| matches!(
        e,
        ConnectEvent::Steps { steps } if steps.contains(&Step::Channel) || steps.contains(&Step::Shell)
    )));
    assert!(!events.iter().any(|e| matches!(
        e,
        ConnectEvent::StepStarted { step: Step::Channel } | ConnectEvent::StepStarted { step: Step::Shell }
    )));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn lost_connection_reconnects_on_the_same_session() {
    use linascp_core::connect::{Connector, Enabled, ReconnectingSession};
    use linascp_core::session::{OutputSink, SessionEvent};
    use std::sync::Mutex;

    let c = Container::start(None);
    let env = Arc::new(Env::new());
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);

    let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
    let (output, collect) = Output::new();
    let sink: OutputSink = {
        let events = events.clone();
        Arc::new(move |event: SessionEvent| {
            if !matches!(event, SessionEvent::Output(_)) {
                events.lock().unwrap().push(event.clone());
            }
            collect(event);
        })
    };
    let connector: Connector = {
        let (env, h) = (env.clone(), h.clone());
        Arc::new(move |cols, rows, sink, cancel| {
            let (env, h) = (env.clone(), h.clone());
            Box::pin(async move {
                let mut req = request(h, None);
                req.cols = cols;
                req.rows = rows;
                let broker = Arc::new(linascp_core::connect::PromptBroker::non_interactive());
                let session = connect_ssh(req, env.deps(None), broker, Arc::new(|_| {}), sink, cancel).await?;
                let info = session.info().clone();
                Ok((Arc::new(session) as Arc<dyn Session>, info))
            })
        })
    };
    let enabled: Enabled = Arc::new(|| true);
    let session = ReconnectingSession::new(sink, 80, 24, connector, enabled);
    // First connect: interactive broker (trusts the unknown host key), wrapper's sink.
    let initial = connect_ssh(
        request(h, None),
        env.deps(None),
        p.broker.clone(),
        p.emit.clone(),
        session.first_sink(),
        CancellationToken::new(),
    )
    .await
    .expect("connected");
    session.attach(Arc::new(initial));

    session.write(b"echo before-$((1+1))\n").await.unwrap();
    output.wait_for("before-2").await;
    // Kill the server side of this login without a regular channel close. The slim image has no
    // procps, so scan /proc; OpenSSH >= 9.8 names the per-login processes `sshd-session: <user> …`.
    // `[:]` keeps the pattern from matching this shell's own command line.
    let kill = r#"for p in /proc/[0-9]*; do grep -qaE 'sshd(-session)?[:] pwuser' "$p/cmdline" 2>/dev/null && kill -9 "${p#/proc/}"; done; true"#;
    let _ = std::process::Command::new("docker").args(["exec", &c.id, "sh", "-c", kill]).output();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::Reconnected { .. })) {
        assert!(std::time::Instant::now() < deadline, "no reconnect: {:?}", events.lock().unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    session.write(b"echo after-$((2+2))\n").await.unwrap();
    output.wait_for("after-4").await;
    session.close().await.unwrap();
}

async fn sftp(
    env: &Env,
    h: Host,
    lost: LostFn,
) -> (linascp_core::connect::SftpSession, linascp_core::sftp::Listing, Vec<ConnectEvent>) {
    let p = prompter(trust_all);
    let (session, listing) = connect_sftp(
        request(h, None),
        env.deps(None),
        p.broker.clone(),
        p.emit.clone(),
        lost,
        CancellationToken::new(),
    )
    .await
    .expect("sftp connected");
    (session, listing, p.events())
}

fn no_lost() -> LostFn {
    Arc::new(|e| panic!("unexpected lost: {e:?}"))
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_connects_into_home_with_sftp_steps() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let (session, listing, events) = sftp(&env, h, no_lost()).await;
    assert_eq!(listing.path, "/home/pwuser");
    assert!(listing.entries.iter().any(|e| e.name == ".bashrc" && e.owner.as_deref() == Some("pwuser")));
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepDone { step: Step::Subsystem })));
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepDone { step: Step::Directory })));
    assert_eq!(session.version(), 3);
    session.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_unreadable_remote_start_fails_the_directory_step_once() {
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.sftp.remote_start = Some("/root".into());
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);
    let result = connect_sftp(
        request(h, None),
        env.deps(None),
        p.broker.clone(),
        p.emit.clone(),
        no_lost(),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(result, Err(CoreError::Fs { kind: FsKind::PermissionDenied, .. })), "{:?}", result.err());
    let events = p.events();
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepFailed { step: Step::Directory, .. })));
    assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_mkdir_rename_and_errors() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let (s, home, _) = sftp(&env, h, no_lost()).await;
    let d = home.path;
    s.mkdir(&d, "a").await.unwrap();
    s.mkdir(&d, "b").await.unwrap();
    assert!(matches!(s.mkdir(&d, "a").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    assert!(matches!(s.rename(&d, "a", "b").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    s.rename(&d, "a", "c").await.unwrap();
    let names: Vec<_> = s.list(&d).await.unwrap().entries.into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"c".into()) && names.contains(&"b".into()) && !names.contains(&"a".into()));
    assert!(matches!(s.list("/root").await, Err(CoreError::Fs { kind: FsKind::PermissionDenied, .. })));
    assert!(matches!(s.mkdir(&d, "x/y").await, Err(CoreError::Config { .. })));
    s.create_file(&d, "new.txt").await.unwrap();
    assert!(matches!(s.create_file(&d, "new.txt").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    assert!(matches!(s.create_file(&d, "b").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    assert!(matches!(s.create_file(&d, "x/y").await, Err(CoreError::Config { .. })));
    s.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_delete_removes_links_not_targets() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let setup = "mkdir -p /home/pwuser/keep /home/pwuser/tree/inner && touch /home/pwuser/keep/k /home/pwuser/tree/inner/x \
        && ln -s /home/pwuser/keep /home/pwuser/tree/link && ln -s / /home/pwuser/rootlink && ln -s loop /home/pwuser/loop \
        && chown -R pwuser: /home/pwuser";
    let out = std::process::Command::new("docker").args(["exec", &c.id, "sh", "-c", setup]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let (s, home, _) = sftp(&env, h, no_lost()).await;
    let kind = |name: &str| home.entries.iter().find(|e| e.name == name).unwrap().kind.clone();
    assert_eq!(kind("rootlink"), EntryKind::Symlink { target: Some(LinkTarget::Dir) });
    assert_eq!(kind("loop"), EntryKind::Symlink { target: None });
    s.delete(&home.path, &["tree".into(), "rootlink".into(), "loop".into()]).await.unwrap();
    let after: Vec<_> = s.list(&home.path).await.unwrap().entries.into_iter().map(|e| e.name).collect();
    assert!(!after.iter().any(|n| n == "tree" || n == "rootlink" || n == "loop"), "{after:?}");
    assert!(s.list("/home/pwuser/keep").await.unwrap().entries.iter().any(|e| e.name == "k"));
    s.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_killed_session_reports_lost_once() {
    use std::sync::Mutex;
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.keepalive_s = 1;
    h.timeout_s = 3;
    env.store_password(&h, "pwpass");
    let lost: Arc<Mutex<Vec<CoreError>>> = Arc::default();
    let on_lost: LostFn = {
        let lost = lost.clone();
        Arc::new(move |e| lost.lock().unwrap().push(e))
    };
    let (s, home, _) = sftp(&env, h, on_lost).await;
    let kill = r#"for p in /proc/[0-9]*; do grep -qaE 'sshd(-session)?[:] pwuser' "$p/cmdline" 2>/dev/null && kill -9 "${p#/proc/}"; done; true"#;
    let _ = std::process::Command::new("docker").args(["exec", &c.id, "sh", "-c", kill]).output();
    let _ = s.list(&home.path).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while lost.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "no lost event");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(matches!(s.list(&home.path).await, Err(CoreError::Network { .. })));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(lost.lock().unwrap().len(), 1);
    s.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_frozen_link_without_keepalive_reports_lost_once() {
    use std::sync::Mutex;
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.keepalive_s = 0;
    h.timeout_s = 2;
    env.store_password(&h, "pwpass");
    let lost: Arc<Mutex<Vec<CoreError>>> = Arc::default();
    let on_lost: LostFn = {
        let lost = lost.clone();
        Arc::new(move |e| lost.lock().unwrap().push(e))
    };
    let (s, home, _) = sftp(&env, h, on_lost).await;
    let pause = std::process::Command::new("docker").args(["pause", &c.id]).output().unwrap();
    assert!(pause.status.success(), "{pause:?}");
    let result = s.list(&home.path).await;
    let _ = std::process::Command::new("docker").args(["unpause", &c.id]).output();
    assert!(matches!(result, Err(CoreError::Network { .. })), "{result:?}");
    assert_eq!(lost.lock().unwrap().len(), 1, "a timed-out request on an open handle reports Lost");
    let _ = s.list(&home.path).await;
    assert_eq!(lost.lock().unwrap().len(), 1);
    s.close().await;
}

mod transfers {
    use super::*;
    use linascp_core::transfer::{Direction, JobState, LocalSide, Side, TransferEvent, TransferOptions, TransferQueue};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    pub(super) fn recorder() -> (Arc<Mutex<Vec<TransferEvent>>>, linascp_core::transfer::TransferEmit) {
        let events: Arc<Mutex<Vec<TransferEvent>>> = Arc::default();
        let emit = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e)) as linascp_core::transfer::TransferEmit
        };
        (events, emit)
    }

    pub(super) async fn wait_batch_done(events: &Arc<Mutex<Vec<TransferEvent>>>, count: usize, secs: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let done = events.lock().unwrap().iter().filter(|e| matches!(e, TransferEvent::BatchDone { .. })).count();
            if done >= count {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "no BatchDone: {:#?}", events.lock().unwrap());
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    pub(super) fn states(events: &Arc<Mutex<Vec<TransferEvent>>>) -> Vec<(String, JobState)> {
        let mut last = std::collections::BTreeMap::new();
        for e in events.lock().unwrap().iter() {
            if let TransferEvent::Job { job } = e {
                last.insert(job.rel.clone(), job.state);
            }
        }
        last.into_iter().collect()
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn upload_and_download_a_tree_with_modes_and_mtime() {
        let c = Container::start(None);
        let env = Env::new();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let (session, home, _) = sftp(&env, h, no_lost()).await;
        let session = Arc::new(session);
        let local = tempfile::tempdir().unwrap();
        let tree = local.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        let big: Vec<u8> = (0..(5 * 1024 * 1024 + 3)).map(|i| (i % 241) as u8).collect();
        std::fs::write(tree.join("big.bin"), &big).unwrap();
        std::fs::write(tree.join("sub/key"), b"secret").unwrap();
        std::fs::set_permissions(tree.join("sub/key"), std::fs::Permissions::from_mode(0o600)).unwrap();
        let (events, emit) = recorder();
        let queue = TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions { parallel: 2, upload_mode: Some(0o644), bandwidth: None },
            emit,
        );
        queue.set_remote(Some(session.clone()));
        queue.enqueue(Direction::Upload, vec![tree.to_str().unwrap().into()], home.path.clone()).await.unwrap();
        wait_batch_done(&events, 1, 60).await;
        assert!(states(&events).iter().all(|(_, s)| *s == JobState::Done), "{:?}", states(&events));
        let remote_tree = format!("{}/tree", home.path);
        let node = session.stat(&format!("{remote_tree}/sub/key")).await.unwrap().unwrap();
        assert_eq!(node.mode & 0o777, 0o644);
        assert_eq!(session.stat(&remote_tree).await.unwrap().unwrap().mode & 0o777, 0o755);
        assert!(session.stat(&format!("{remote_tree}/big.bin.filepart")).await.unwrap().is_none());

        let back = tempfile::tempdir().unwrap();
        queue
            .enqueue(Direction::Download, vec![remote_tree.clone()], back.path().to_str().unwrap().into())
            .await
            .unwrap();
        wait_batch_done(&events, 2, 60).await;
        assert_eq!(std::fs::read(back.path().join("tree/big.bin")).unwrap(), big);
        let local_mtime = std::fs::metadata(tree.join("big.bin")).unwrap().modified().unwrap();
        let back_mtime = std::fs::metadata(back.path().join("tree/big.bin")).unwrap().modified().unwrap();
        assert_eq!(
            local_mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            back_mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        );
        assert_eq!(std::fs::metadata(back.path().join("tree/sub/key")).unwrap().permissions().mode() & 0o777, 0o644);
        queue.close().await;
        session.close().await;
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn overwrite_replaces_an_existing_remote_file() {
        use linascp_core::transfer::Decision;
        let c = Container::start(None);
        let env = Env::new();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let (session, home, _) = sftp(&env, h, no_lost()).await;
        assert!(session.posix_rename(), "OpenSSH offers posix-rename@openssh.com");
        let session = Arc::new(session);
        let local = tempfile::tempdir().unwrap();
        std::fs::write(local.path().join("f"), b"first").unwrap();
        let (events, emit) = recorder();
        let queue = TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions { parallel: 1, upload_mode: None, bandwidth: None },
            emit,
        );
        queue.set_remote(Some(session.clone()));
        let src = vec![local.path().join("f").to_str().unwrap().to_string()];
        queue.enqueue(Direction::Upload, src.clone(), home.path.clone()).await.unwrap();
        wait_batch_done(&events, 1, 30).await;
        std::fs::write(local.path().join("f"), b"second!").unwrap();
        queue.enqueue(Direction::Upload, src, home.path.clone()).await.unwrap();
        let job_id = loop {
            let id = events.lock().unwrap().iter().find_map(|e| match e {
                TransferEvent::Conflict { job_id, .. } => Some(*job_id),
                _ => None,
            });
            if let Some(id) = id {
                break id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        queue.resolve(job_id, Decision::Overwrite, false).unwrap();
        wait_batch_done(&events, 2, 30).await;
        let node = session.stat(&format!("{}/f", home.path)).await.unwrap().unwrap();
        assert_eq!(node.size, 7);
        queue.close().await;
        session.close().await;
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn a_frozen_link_reconnects_and_the_upload_finishes_identical() {
        use linascp_core::connect::{ConnConnector, ConnStatus, PromptBroker, Reconnecting, SftpSession};
        let c = Container::start(None);
        let env = Arc::new(Env::new());
        let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
        h.keepalive_s = 1;
        h.timeout_s = 3;
        env.store_password(&h, "pwpass");
        // Trust the host key once (interactive), so the non-interactive attempts can connect.
        let (first, home, _) = sftp(&env, h.clone(), Arc::new(|_| {})).await;
        first.close().await;

        let (events, record) = recorder();
        // Freeze the server inside the first progress callback: it runs on the copy path, so the
        // file cannot finish before the container is paused (a local upload outruns any polling).
        let frozen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let emit: linascp_core::transfer::TransferEmit = {
            let (frozen, id) = (frozen.clone(), c.id.clone());
            Arc::new(move |e| {
                if matches!(e, TransferEvent::Progress { .. })
                    && !frozen.swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    let pause = std::process::Command::new("docker").args(["pause", &id]).output().unwrap();
                    assert!(pause.status.success(), "{pause:?}");
                }
                record(e)
            })
        };
        let queue = TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions { parallel: 1, upload_mode: None, bandwidth: None },
            emit,
        );
        let statuses: Arc<Mutex<Vec<ConnStatus>>> = Arc::default();
        let connector: ConnConnector<SftpSession> = {
            let (env, h) = (env.clone(), h.clone());
            Arc::new(move |lost, cancel| {
                let (env, h) = (env.clone(), h.clone());
                Box::pin(async move {
                    let broker = Arc::new(PromptBroker::non_interactive());
                    let (s, _) =
                        connect_sftp(request(h, None), env.deps(None), broker, Arc::new(|_| {}), lost, cancel).await?;
                    Ok(Arc::new(s))
                })
            })
        };
        let rc = Reconnecting::new(
            connector.clone(),
            Arc::new(|| true),
            {
                let s = statuses.clone();
                Arc::new(move |st| s.lock().unwrap().push(st))
            },
            {
                let q = queue.clone();
                Arc::new(move |conn: Option<Arc<SftpSession>>| q.set_remote(conn.map(|c| c as Arc<dyn Side>)))
            },
        );
        let (generation, lost) = rc.begin();
        let session = connector(lost, CancellationToken::new()).await.expect("connected");
        rc.attach(generation, session);

        let local = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(64 * 1024 * 1024)).map(|i| (i % 249) as u8).collect();
        std::fs::write(local.path().join("big.bin"), &data).unwrap();
        queue
            .enqueue(Direction::Upload, vec![local.path().join("big.bin").to_str().unwrap().into()], home.path.clone())
            .await
            .unwrap();
        // Keep it frozen for longer than timeout + keepalive.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !frozen.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(std::time::Instant::now() < deadline, "no progress: {:?}", events.lock().unwrap());
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let _ = std::process::Command::new("docker").args(["unpause", &c.id]).output();
        wait_batch_done(&events, 1, 120).await;
        assert!(
            statuses.lock().unwrap().iter().any(|s| matches!(s, ConnStatus::Reconnected { .. })),
            "{:?}",
            statuses.lock().unwrap()
        );
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, TransferEvent::Job { job } if job.state == JobState::Paused))
        );
        assert_eq!(states(&events), vec![("big.bin".to_string(), JobState::Done)]);
        let back = tempfile::tempdir().unwrap();
        queue
            .enqueue(Direction::Download, vec![format!("{}/big.bin", home.path)], back.path().to_str().unwrap().into())
            .await
            .unwrap();
        wait_batch_done(&events, 2, 120).await;
        assert!(std::fs::read(back.path().join("big.bin")).unwrap() == data, "content differs after resume");
        queue.close().await;
        rc.close().await;
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn edit_roundtrip_against_sshd() {
        use linascp_core::edit::{EditEvent, EditManager, EditState, Launcher};
        struct Remember(Mutex<Option<std::path::PathBuf>>);
        impl Launcher for Remember {
            fn open(&self, path: &std::path::Path, _app: Option<&str>) -> Result<()> {
                *self.0.lock().unwrap() = Some(path.to_path_buf());
                Ok(())
            }
        }
        let c = Container::start(None);
        let env = Env::new();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let (session, home, _) = sftp(&env, h, no_lost()).await;
        let session = Arc::new(session);
        let path = format!("{}/app.conf", home.path);
        let writer = session.open_write(&path, false).await.unwrap();
        writer.write_at(0, b"port=80\n".to_vec()).await.unwrap();
        writer.close().await.unwrap();
        session.set_mode(&path, 0o640).await.unwrap();

        let (_, emit) = recorder();
        let queue = TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions { parallel: 2, upload_mode: Some(0o600), bandwidth: None },
            emit,
        );
        queue.set_remote(Some(session.clone()));
        let cache = tempfile::tempdir().unwrap();
        let launcher = Arc::new(Remember(Mutex::new(None)));
        let states: Arc<Mutex<Vec<EditState>>> = Arc::default();
        let sink = states.clone();
        let edits = EditManager::new(
            queue.clone(),
            cache.path().join("edit"),
            launcher.clone(),
            Arc::new(move |EditEvent::File { file }| sink.lock().unwrap().push(file.state)),
            std::time::Duration::from_millis(100),
        );
        edits.open(&path).await.unwrap();
        let copy = launcher.0.lock().unwrap().clone().unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"port=80\n");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        std::fs::write(&copy, b"port=8080\n").unwrap();
        for _ in 0..300 {
            if states.lock().unwrap().ends_with(&[EditState::Uploading, EditState::Synced]) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(
            states.lock().unwrap().ends_with(&[EditState::Uploading, EditState::Synced]),
            "{:?}",
            states.lock().unwrap()
        );
        let reader = session.open_read(&path).await.unwrap();
        assert_eq!(reader.read_at(0, 64).await.unwrap(), b"port=8080\n");
        reader.close().await;
        assert_eq!(session.stat(&path).await.unwrap().unwrap().mode & 0o777, 0o640);
        assert_eq!(edits.close().await, Vec::<std::path::PathBuf>::new());
        queue.close().await;
        session.close().await;
    }
}
