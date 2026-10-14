//! SSH pipeline against real OpenSSH servers in Docker.
//! Run: cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1
//! Needs: docker, ssh-keygen, ssh-agent, ssh-add.

mod support;

use std::sync::Arc;

use linascp_core::connect::{
    ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, HostKeyDecision, Prompt, PromptAnswer, SshSession, Step,
    connect_ssh, test_connection,
};
use linascp_core::error::{CoreError, NetworkKind, Result};
use linascp_core::known_hosts::{HostKeyStatus, KnownHosts};
use linascp_core::model::{AuthMethod, EnvVar, Host};
use linascp_core::secrets::{MemorySecretStore, SecretKind, SecretOwner, SecretStore};
use linascp_core::session::Session;
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
    assert_shell(session.expect("connected with key + keyboard-interactive"), &output).await;
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
    assert_shell(session.expect("connected"), &output).await;
    assert!(matches!(p.prompts()[0], Prompt::HostKeyUnknown { .. }));
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
