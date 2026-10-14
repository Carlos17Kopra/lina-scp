//! Execution of the configured SSH authentication method.

use std::path::Path;
use std::sync::Arc;

use russh::MethodSet;
use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::AgentClient;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

use super::classify::map_russh_error;
use super::handler::ClientHandler;
use super::prompt::{EmitFn, KbdPrompt, Prompt, PromptAnswer, PromptBroker};
use super::{ConnectTarget, log_line};
use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, AuthMethod, HostAuth};
use crate::paths::expand_tilde;
use crate::secrets::{SecretKind, SecretOwner, SecretStore};

/// Maximum password / passphrase attempts before giving up.
pub const MAX_ATTEMPTS: usize = 3;
/// Upper bound for keyboard-interactive info request rounds.
const MAX_KBD_ROUNDS: usize = 16;
/// Upper bound for chained methods after partial success (`AuthenticationMethods a,b,c`).
/// `MAX_ATTEMPTS` still applies per method.
const MAX_METHOD_ROUNDS: usize = 4;

/// Result of a single authentication request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Success,
    /// Accepted, but the server requires more (partial success); `remaining` = methods to continue with.
    Partial {
        remaining: Vec<String>,
    },
    /// Rejected; `remaining` = methods the server still accepts.
    Rejected {
        remaining: Vec<String>,
    },
}

/// One step of a keyboard-interactive exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbdStep {
    Success,
    /// Accepted with partial success; `remaining` = methods to continue with.
    Partial {
        remaining: Vec<String>,
    },
    Rejected {
        remaining: Vec<String>,
    },
    Info {
        name: String,
        instructions: String,
        prompts: Vec<KbdPrompt>,
    },
}

/// How one method ended when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MethodOutcome {
    Done,
    /// Partial success: continue with one of these methods.
    Partial(Vec<String>),
}

/// Wire name of a method (as in `MethodSet` / `AuthenticationMethods`).
fn wire_name(method: AuthMethod) -> &'static str {
    match method {
        AuthMethod::Password => "password",
        AuthMethod::Key | AuthMethod::Agent => "publickey",
        AuthMethod::KeyboardInteractive => "keyboard-interactive",
    }
}

/// Next method after a partial success: keyboard-interactive, then password, then the configured
/// publickey method (key file or agent). A method that already succeeded is not repeated.
fn next_method(remaining: &[String], configured: AuthMethod, used: &[AuthMethod]) -> Option<AuthMethod> {
    let mut candidates = vec![AuthMethod::KeyboardInteractive, AuthMethod::Password];
    if matches!(configured, AuthMethod::Key | AuthMethod::Agent) {
        candidates.push(configured);
    }
    candidates
        .into_iter()
        .find(|candidate| !used.contains(candidate) && remaining.iter().any(|m| m == wire_name(*candidate)))
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
    if let Some(user) = target.profile.as_ref().map(|p| p.user.clone()).filter(non_empty) {
        return Ok(user);
    }
    std::env::var("USER")
        .ok()
        .filter(non_empty)
        .ok_or_else(|| CoreError::Config { message: format!("no user configured for host '{}'", target.host.name) })
}

/// Auth settings and secret owner (profile or host) for a target.
pub fn resolve_auth(target: &ConnectTarget) -> Result<(AuthConfig, SecretOwner)> {
    match &target.host.auth {
        HostAuth::Custom { auth } => Ok((auth.clone(), SecretOwner::Host(target.host.id))),
        HostAuth::Profile { profile_id } => match &target.profile {
            Some(profile) if profile.id == *profile_id => Ok((profile.auth.clone(), SecretOwner::Profile(profile.id))),
            _ => Err(CoreError::Config { message: format!("login profile {profile_id} not found") }),
        },
    }
}

fn method_names(set: &MethodSet) -> Vec<String> {
    set.iter().map(String::from).collect()
}

fn auth_error(remaining: Vec<String>, fallback: &str, message: &str) -> CoreError {
    let rejected_methods = if remaining.is_empty() { vec![fallback.to_string()] } else { remaining };
    CoreError::Auth { rejected_methods, message: message.to_string() }
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
            .unwrap_or_else(|e| Err(CoreError::Internal { message: e.to_string() }));
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
            .unwrap_or_else(|e| Err(CoreError::Internal { message: e.to_string() }));
        if let Err(e) = result {
            self.log(format!("debug1: could not save secret in keyring: {e}"));
        }
    }

    /// Ask for a password/passphrase; a cancelled prompt aborts the connect.
    async fn ask_secret(&self, prompt: Prompt) -> Result<(SecretString, bool)> {
        match self.broker.ask_cancellable(self.emit, prompt, self.cancel).await {
            PromptAnswer::Secret { value, save } => Ok((SecretString::from(value), save)),
            _ => Err(CoreError::Cancelled),
        }
    }
}

async fn run_method(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>, method: AuthMethod) -> Result<MethodOutcome> {
    match method {
        AuthMethod::Password => password_flow(auth, ctx).await,
        AuthMethod::Key => key_flow(auth, ctx).await,
        AuthMethod::Agent => agent_flow(auth, ctx).await,
        AuthMethod::KeyboardInteractive => kbd_flow(auth, ctx).await,
    }
}

/// Run the configured method (with retries where applicable). On partial success (e.g.
/// `AuthenticationMethods publickey,keyboard-interactive`) continue with the next method the
/// server still accepts, see [`next_method`]; all of it stays within the Auth step.
pub async fn authenticate(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<()> {
    ctx.log(format!("debug1: Authenticating as '{}' using {:?}", ctx.user, ctx.auth.method));
    let mut method = ctx.auth.method;
    let mut used = Vec::new();
    for _ in 0..MAX_METHOD_ROUNDS {
        used.push(method);
        match run_method(auth, ctx, method).await? {
            MethodOutcome::Done => {
                ctx.log(format!("debug1: Authenticated as '{}'", ctx.user));
                return Ok(());
            }
            MethodOutcome::Partial(remaining) => {
                ctx.log(format!(
                    "debug1: Authenticated using {} with partial success; can continue: {}",
                    wire_name(method),
                    remaining.join(",")
                ));
                match next_method(&remaining, ctx.auth.method, &used) {
                    Some(next) => method = next,
                    None => {
                        return Err(auth_error(
                            remaining,
                            wire_name(method),
                            "server requires a further authentication method that is not configured",
                        ));
                    }
                }
            }
        }
    }
    Err(CoreError::Auth {
        rejected_methods: vec![wire_name(method).to_string()],
        message: "too many authentication rounds".to_string(),
    })
}

async fn password_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<MethodOutcome> {
    let mut stored = ctx.stored_secret(SecretKind::Password).await;
    let mut remaining = Vec::new();
    for attempt in 0..MAX_ATTEMPTS {
        let (password, save) = match stored.take() {
            Some(secret) => (secret, false),
            None => {
                ctx.ask_secret(Prompt::Password { user: ctx.user.clone(), host: ctx.host.clone(), retry: attempt > 0 })
                    .await?
            }
        };
        match auth.password(&ctx.user, &password).await? {
            AuthOutcome::Success => {
                if save {
                    ctx.save_secret(SecretKind::Password, password).await;
                }
                return Ok(MethodOutcome::Done);
            }
            AuthOutcome::Partial { remaining } => {
                // The password itself was accepted.
                if save {
                    ctx.save_secret(SecretKind::Password, password).await;
                }
                return Ok(MethodOutcome::Partial(remaining));
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
    Err(auth_error(remaining, "password", "password authentication failed"))
}

fn decode_key(text: &str, passphrase: Option<&str>) -> std::result::Result<PrivateKey, russh::keys::Error> {
    russh::keys::decode_secret_key(text, passphrase)
}

async fn load_key(ctx: &AuthContext<'_>, key_path: &str) -> Result<PrivateKey> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = expand_tilde(key_path, Path::new(&home));
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| CoreError::Config { message: format!("cannot read key file {}: {e}", path.display()) })?;
    match decode_key(&text, None) {
        Ok(key) => return Ok(key),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => {
            return Err(CoreError::Config { message: format!("invalid key file {}: {e}", path.display()) });
        }
    }
    let mut stored = ctx.stored_secret(SecretKind::Passphrase).await;
    for _ in 0..MAX_ATTEMPTS {
        let (passphrase, save) = match stored.take() {
            Some(secret) => (secret, false),
            None => ctx.ask_secret(Prompt::Passphrase { key_path: key_path.to_string() }).await?,
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

async fn key_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<MethodOutcome> {
    let key_path =
        ctx.auth.key_path.clone().ok_or_else(|| CoreError::Config { message: "no key file configured".to_string() })?;
    let key = load_key(ctx, &key_path).await?;
    ctx.log(format!("debug1: Offering public key: {key_path}"));
    match auth.publickey(&ctx.user, key).await? {
        AuthOutcome::Success => Ok(MethodOutcome::Done),
        AuthOutcome::Partial { remaining } => Ok(MethodOutcome::Partial(remaining)),
        AuthOutcome::Rejected { remaining } => Err(auth_error(remaining, "publickey", "public key was rejected")),
    }
}

async fn agent_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<MethodOutcome> {
    let socket = ctx.agent_sock.ok_or_else(|| CoreError::Auth {
        rejected_methods: vec!["publickey".to_string()],
        message: "no ssh-agent available (SSH_AUTH_SOCK not set)".to_string(),
    })?;
    match auth.agent(&ctx.user, socket).await? {
        AuthOutcome::Success => Ok(MethodOutcome::Done),
        AuthOutcome::Partial { remaining } => Ok(MethodOutcome::Partial(remaining)),
        AuthOutcome::Rejected { remaining } => {
            Err(auth_error(remaining, "publickey", "no agent identity was accepted"))
        }
    }
}

async fn kbd_flow(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<MethodOutcome> {
    let mut step = auth.kbd_start(&ctx.user).await?;
    for _ in 0..MAX_KBD_ROUNDS {
        match step {
            KbdStep::Success => return Ok(MethodOutcome::Done),
            KbdStep::Partial { remaining } => return Ok(MethodOutcome::Partial(remaining)),
            KbdStep::Rejected { remaining } => {
                return Err(auth_error(
                    remaining,
                    "keyboard-interactive",
                    "keyboard-interactive authentication failed",
                ));
            }
            KbdStep::Info { name, instructions, prompts } => {
                let responses = if prompts.is_empty() {
                    Vec::new()
                } else {
                    let expected = prompts.len();
                    match ctx
                        .broker
                        .ask_cancellable(
                            ctx.emit,
                            Prompt::KeyboardInteractive { name, instructions, prompts },
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
        client::AuthResult::Failure { remaining_methods, partial_success: true } => {
            AuthOutcome::Partial { remaining: method_names(&remaining_methods) }
        }
        client::AuthResult::Failure { remaining_methods, partial_success: false } => {
            AuthOutcome::Rejected { remaining: method_names(&remaining_methods) }
        }
    }
}

fn kbd_step(response: KeyboardInteractiveAuthResponse) -> KbdStep {
    match response {
        KeyboardInteractiveAuthResponse::Success => KbdStep::Success,
        KeyboardInteractiveAuthResponse::Failure { remaining_methods, partial_success: true } => {
            KbdStep::Partial { remaining: method_names(&remaining_methods) }
        }
        KeyboardInteractiveAuthResponse::Failure { remaining_methods, partial_success: false } => {
            KbdStep::Rejected { remaining: method_names(&remaining_methods) }
        }
        KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => KbdStep::Info {
            name,
            instructions,
            prompts: prompts.into_iter().map(|p| KbdPrompt { text: p.prompt, echo: p.echo }).collect(),
        },
    }
}

impl HandleAuthenticator<'_> {
    async fn rsa_hash(&self) -> Result<Option<HashAlg>> {
        let best = self.handle.best_supported_rsa_hash().await.map_err(map_russh_error)?;
        // Server did not announce server-sig-algs: assume rsa-sha2-256.
        Ok(best.unwrap_or(Some(HashAlg::Sha256)))
    }
}

#[async_trait::async_trait]
impl Authenticator for HandleAuthenticator<'_> {
    async fn password(&mut self, user: &str, password: &SecretString) -> Result<AuthOutcome> {
        self.handle.authenticate_password(user, password.expose_secret()).await.map(outcome).map_err(map_russh_error)
    }

    async fn publickey(&mut self, user: &str, key: PrivateKey) -> Result<AuthOutcome> {
        let hash = if key.algorithm().is_rsa() { self.rsa_hash().await? } else { None };
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
        self.handle.authenticate_publickey(user, key).await.map(outcome).map_err(map_russh_error)
    }

    async fn agent(&mut self, user: &str, socket: &Path) -> Result<AuthOutcome> {
        let agent_error = |e: russh::keys::Error| CoreError::Auth {
            rejected_methods: vec!["publickey".to_string()],
            message: format!("ssh-agent: {e}"),
        };
        let mut agent = AgentClient::connect_uds(socket).await.map_err(agent_error)?;
        let identities = agent.request_identities().await.map_err(agent_error)?;
        let mut last = AuthOutcome::Rejected { remaining: Vec::new() };
        for identity in identities {
            let public = identity.public_key().into_owned();
            let hash = if public.algorithm().is_rsa() { self.rsa_hash().await? } else { None };
            let result =
                self.handle.authenticate_publickey_with(user, public, hash, &mut agent).await.map_err(|e| {
                    CoreError::Auth {
                        rejected_methods: vec!["publickey".to_string()],
                        message: format!("ssh-agent: {e}"),
                    }
                })?;
            last = outcome(result);
            // Success or partial success: this identity was accepted.
            if !matches!(last, AuthOutcome::Rejected { .. }) {
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
        self.handle.authenticate_keyboard_interactive_respond(responses).await.map(kbd_step).map_err(map_russh_error)
    }
}

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
        /// Scripted publickey results; `Success` once exhausted.
        publickey_results: VecDeque<AuthOutcome>,
        /// Order in which methods were tried.
        calls: Vec<&'static str>,
        kbd_steps: VecDeque<KbdStep>,
        kbd_responses: Vec<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl Authenticator for FakeAuth {
        async fn password(&mut self, _user: &str, password: &SecretString) -> Result<AuthOutcome> {
            self.calls.push("password");
            self.passwords_seen.push(password.expose_secret().to_string());
            Ok(self.password_results.pop_front().expect("scripted result"))
        }
        async fn publickey(&mut self, _user: &str, _key: PrivateKey) -> Result<AuthOutcome> {
            self.calls.push("publickey");
            self.publickey_calls += 1;
            Ok(self.publickey_results.pop_front().unwrap_or(AuthOutcome::Success))
        }
        async fn agent(&mut self, _user: &str, _socket: &Path) -> Result<AuthOutcome> {
            Ok(AuthOutcome::Rejected { remaining: vec!["publickey".into()] })
        }
        async fn kbd_start(&mut self, _user: &str) -> Result<KbdStep> {
            self.calls.push("keyboard-interactive");
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
                    let answer = queue.lock().unwrap().pop_front().unwrap_or(PromptAnswer::Cancel);
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
                auth: AuthConfig { method, key_path, has_secret: false },
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
        PromptAnswer::Secret { value: value.into(), save }
    }

    fn rejected(methods: &[&str]) -> AuthOutcome {
        AuthOutcome::Rejected { remaining: methods.iter().map(|m| m.to_string()).collect() }
    }

    #[test]
    fn effective_user_prefers_host_then_profile() {
        let mut host = Host::new("web", "web.example.org");
        let profile = LoginProfile {
            id: uuid::Uuid::new_v4(),
            name: "ops".into(),
            user: "ops".into(),
            auth: AuthConfig { method: AuthMethod::Agent, key_path: None, has_secret: false },
            created_at: 0,
            updated_at: 0,
        };
        let mut target = ConnectTarget { host: host.clone(), profile: Some(profile.clone()) };
        assert_eq!(effective_user(&target).unwrap(), "ops");
        host.user = Some("root".into());
        target.host = host;
        assert_eq!(effective_user(&target).unwrap(), "root");
    }

    #[test]
    fn resolve_auth_owner_is_profile_or_host() {
        let mut host = Host::new("web", "web.example.org");
        let target = ConnectTarget { host: host.clone(), profile: None };
        let (auth, owner) = resolve_auth(&target).unwrap();
        assert_eq!(auth.method, AuthMethod::Password);
        assert_eq!(owner, SecretOwner::Host(host.id));

        let profile = LoginProfile {
            id: uuid::Uuid::new_v4(),
            name: "ops".into(),
            user: "ops".into(),
            auth: AuthConfig { method: AuthMethod::Key, key_path: Some("~/.ssh/id_ed25519".into()), has_secret: true },
            created_at: 0,
            updated_at: 0,
        };
        host.auth = HostAuth::Profile { profile_id: profile.id };
        let target = ConnectTarget { host: host.clone(), profile: Some(profile.clone()) };
        let (auth, owner) = resolve_auth(&target).unwrap();
        assert_eq!(auth.method, AuthMethod::Key);
        assert_eq!(owner, SecretOwner::Profile(profile.id));

        let missing = ConnectTarget { host, profile: None };
        assert!(matches!(resolve_auth(&missing), Err(CoreError::Config { .. })));
    }

    #[tokio::test]
    async fn password_from_store_needs_no_prompt() {
        let h = Harness::new(vec![]);
        h.secrets.set(h.owner, SecretKind::Password, &SecretString::from("stored".to_string())).unwrap();
        let mut fake = FakeAuth { password_results: VecDeque::from([AuthOutcome::Success]), ..Default::default() };
        authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap();
        assert_eq!(fake.passwords_seen, vec!["stored"]);
        assert!(h.prompts().is_empty());
    }

    #[tokio::test]
    async fn prompted_password_is_saved_on_success() {
        let h = Harness::new(vec![secret("typed", true)]);
        let mut fake = FakeAuth { password_results: VecDeque::from([AuthOutcome::Success]), ..Default::default() };
        authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap();
        assert_eq!(
            h.prompts(),
            vec![Prompt::Password { user: "alice".into(), host: "example.org".into(), retry: false }]
        );
        let saved = h.secrets.get(h.owner, SecretKind::Password).unwrap().unwrap();
        assert_eq!(saved.expose_secret(), "typed");
    }

    #[tokio::test]
    async fn password_gives_up_after_three_attempts() {
        let h = Harness::new(vec![secret("b", true), secret("c", false)]);
        h.secrets.set(h.owner, SecretKind::Password, &SecretString::from("a".to_string())).unwrap();
        let remaining = ["publickey", "password"];
        let mut fake = FakeAuth {
            password_results: VecDeque::from([rejected(&remaining), rejected(&remaining), rejected(&remaining)]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap_err();
        match err {
            CoreError::Auth { rejected_methods, .. } => assert_eq!(rejected_methods, vec!["publickey", "password"]),
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
        assert_eq!(h.secrets.get(h.owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "a");
    }

    #[tokio::test]
    async fn password_stops_when_server_drops_password_method() {
        let h = Harness::new(vec![secret("x", false)]);
        let mut fake = FakeAuth { password_results: VecDeque::from([rejected(&["publickey"])]), ..Default::default() };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap_err();
        assert!(matches!(err, CoreError::Auth { .. }));
        assert_eq!(fake.passwords_seen.len(), 1);
    }

    #[tokio::test]
    async fn password_prompt_cancel_aborts() {
        let h = Harness::new(vec![PromptAnswer::Cancel]);
        let mut fake = FakeAuth::default();
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap_err();
        assert!(matches!(err, CoreError::Cancelled));
    }

    #[tokio::test]
    async fn encrypted_key_retries_passphrase_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("id_ed25519");
        std::fs::write(&key_path, ENCRYPTED_KEY).unwrap();
        let h = Harness::new(vec![secret("wrong", true), secret("secret", true)]);
        let mut fake = FakeAuth::default();
        authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key_path.display().to_string()))).await.unwrap();
        assert_eq!(fake.publickey_calls, 1);
        assert_eq!(h.prompts().len(), 2);
        assert!(matches!(h.prompts()[0], Prompt::Passphrase { .. }));
        let saved = h.secrets.get(h.owner, SecretKind::Passphrase).unwrap().unwrap();
        assert_eq!(saved.expose_secret(), "secret");
    }

    #[tokio::test]
    async fn missing_key_file_is_config_error() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth::default();
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some("/nonexistent/linascp/key".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }));
    }

    #[tokio::test]
    async fn agent_without_socket_fails() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth::default();
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Agent, None)).await.unwrap_err();
        assert!(matches!(err, CoreError::Auth { .. }));
    }

    #[tokio::test]
    async fn keyboard_interactive_round_trip() {
        let h = Harness::new(vec![PromptAnswer::Responses { values: vec!["123456".into()] }]);
        let mut fake = FakeAuth {
            kbd_steps: VecDeque::from([
                KbdStep::Info { name: String::new(), instructions: String::new(), prompts: vec![] },
                KbdStep::Info {
                    name: "2FA".into(),
                    instructions: "Enter code".into(),
                    prompts: vec![KbdPrompt { text: "Code: ".into(), echo: true }],
                },
                KbdStep::Success,
            ]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::KeyboardInteractive, None)).await.unwrap();
        assert_eq!(fake.kbd_responses, vec![vec![], vec!["123456".to_string()]]);
        assert_eq!(h.prompts().len(), 1);
    }

    #[tokio::test]
    async fn keyboard_interactive_failure_reports_methods() {
        let h = Harness::new(vec![]);
        let mut fake = FakeAuth {
            kbd_steps: VecDeque::from([KbdStep::Rejected { remaining: vec!["keyboard-interactive".into()] }]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::KeyboardInteractive, None)).await.unwrap_err();
        match err {
            CoreError::Auth { rejected_methods, .. } => assert_eq!(rejected_methods, vec!["keyboard-interactive"]),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn partial(methods: &[&str]) -> AuthOutcome {
        AuthOutcome::Partial { remaining: methods.iter().map(|m| m.to_string()).collect() }
    }

    fn key_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("id_ed25519");
        std::fs::write(&key_path, ENCRYPTED_KEY).unwrap();
        let path = key_path.display().to_string();
        (dir, path)
    }

    fn code_prompt() -> KbdStep {
        KbdStep::Info {
            name: "2FA".into(),
            instructions: String::new(),
            prompts: vec![KbdPrompt { text: "Verification code: ".into(), echo: false }],
        }
    }

    #[tokio::test]
    async fn key_with_partial_success_continues_with_keyboard_interactive() {
        let (_dir, key) = key_file();
        let h = Harness::new(vec![secret("secret", false), PromptAnswer::Responses { values: vec!["123456".into()] }]);
        let mut fake = FakeAuth {
            publickey_results: VecDeque::from([partial(&["password", "keyboard-interactive"])]),
            kbd_steps: VecDeque::from([code_prompt(), KbdStep::Success]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap();
        // keyboard-interactive is preferred over password when both remain.
        assert_eq!(fake.calls, vec!["publickey", "keyboard-interactive"]);
        assert_eq!(fake.kbd_responses, vec![vec!["123456".to_string()]]);
        assert!(matches!(h.prompts()[1], Prompt::KeyboardInteractive { .. }));
    }

    #[tokio::test]
    async fn key_with_partial_success_continues_with_password_and_keeps_its_retries() {
        let (_dir, key) = key_file();
        let h = Harness::new(vec![secret("secret", false), secret("wrong", false), secret("right", true)]);
        let mut fake = FakeAuth {
            publickey_results: VecDeque::from([partial(&["password"])]),
            password_results: VecDeque::from([rejected(&["password"]), AuthOutcome::Success]),
            ..Default::default()
        };
        authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap();
        assert_eq!(fake.calls, vec!["publickey", "password", "password"]);
        assert!(matches!(h.prompts()[2], Prompt::Password { retry: true, .. }));
        assert_eq!(h.secrets.get(h.owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "right");
    }

    #[tokio::test]
    async fn password_with_partial_success_continues_with_the_configured_key_only() {
        // Configured: password. publickey remains but no key/agent is configured -> fails with
        // the server's remaining methods.
        let h = Harness::new(vec![secret("pw", false)]);
        let mut fake = FakeAuth { password_results: VecDeque::from([partial(&["publickey"])]), ..Default::default() };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap_err();
        match err {
            CoreError::Auth { rejected_methods, .. } => assert_eq!(rejected_methods, vec!["publickey"]),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(fake.publickey_calls, 0);
    }

    #[tokio::test]
    async fn partial_success_with_unsupported_remaining_method_is_an_auth_error() {
        let (_dir, key) = key_file();
        let h = Harness::new(vec![secret("secret", false)]);
        let mut fake = FakeAuth { publickey_results: VecDeque::from([partial(&["hostbased"])]), ..Default::default() };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap_err();
        match err {
            CoreError::Auth { rejected_methods, .. } => assert_eq!(rejected_methods, vec!["hostbased"]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn chained_partial_successes_never_repeat_a_method() {
        let (_dir, key) = key_file();
        let h = Harness::new(vec![
            secret("secret", false),
            PromptAnswer::Responses { values: vec!["1".into()] },
            secret("pw", false),
        ]);
        let mut fake = FakeAuth {
            publickey_results: VecDeque::from([partial(&["keyboard-interactive"])]),
            kbd_steps: VecDeque::from([code_prompt(), KbdStep::Partial { remaining: vec!["password".into()] }]),
            password_results: VecDeque::from([partial(&["publickey", "keyboard-interactive", "password"])]),
            ..Default::default()
        };
        let err = authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap_err();
        assert!(matches!(err, CoreError::Auth { .. }));
        assert_eq!(fake.calls, vec!["publickey", "keyboard-interactive", "password"]);
    }

    #[test]
    fn next_method_order_and_configuration() {
        let remaining = |m: &[&str]| m.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let all = remaining(&["publickey", "password", "keyboard-interactive"]);
        assert_eq!(next_method(&all, AuthMethod::Key, &[]), Some(AuthMethod::KeyboardInteractive));
        assert_eq!(next_method(&all, AuthMethod::Key, &[AuthMethod::KeyboardInteractive]), Some(AuthMethod::Password));
        assert_eq!(
            next_method(&all, AuthMethod::Agent, &[AuthMethod::KeyboardInteractive, AuthMethod::Password]),
            Some(AuthMethod::Agent)
        );
        assert_eq!(next_method(&remaining(&["publickey"]), AuthMethod::Password, &[AuthMethod::Password]), None);
        assert_eq!(next_method(&remaining(&["publickey"]), AuthMethod::Key, &[AuthMethod::Key]), None);
    }

    struct LockedKeyring;

    impl crate::secrets::SecretStore for LockedKeyring {
        fn get(&self, _: SecretOwner, _: SecretKind) -> std::result::Result<Option<SecretString>, CoreError> {
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
        let mut fake = FakeAuth { password_results: VecDeque::from([AuthOutcome::Success]), ..Default::default() };
        authenticate(&mut fake, &ctx).await.unwrap();
        assert_eq!(fake.passwords_seen, vec!["typed"]);
        assert_eq!(h.prompts().len(), 1);
    }
}
