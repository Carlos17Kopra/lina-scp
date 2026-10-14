//! `.linascp` export/import: JSON document, secrets (and optionally key files) embedded
//! as an ASCII-armored age block encrypted with a passphrase (scrypt).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::iter;
use std::path::{Path, PathBuf};

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::fsutil;
use crate::model::{AuthConfig, AuthMethod, Group, Host, HostAuth, Id, LoginProfile, now_unix};
use crate::paths::expand_tilde;
use crate::secrets::{SecretKind, SecretOwner, SecretStore, account_name, parse_account_name};
use crate::store::{self, Store};

pub const FORMAT_VERSION: u32 = 1;

/// Fixed scrypt work factor (log2 N) used to encrypt the secrets block, instead of age's
/// default of calibrating to the exporting machine's speed (~1s locally): a file made on a
/// fast desktop must still decrypt on a slower laptop, since `age::scrypt::Identity`
/// rejects stanzas whose work factor exceeds its own calibrated maximum. ~256 MiB / ~1s on
/// modern CPUs.
const SCRYPT_WORK_FACTOR: u8 = 18;

/// Max scrypt work factor accepted on decrypt. Must stay >= `SCRYPT_WORK_FACTOR` so every
/// file we produce is always accepted regardless of the importing machine's speed, while
/// absurdly expensive (hostile) factors are still refused.
const SCRYPT_MAX_WORK_FACTOR: u8 = 20;

/// Settings that travel with an export. `ssh_mode` is excluded on purpose: switching it
/// has side effects on `~/.ssh` that need the user's confirmation.
const EXPORTED_SETTINGS: &[&str] = &[store::THEME, store::TERMINAL_RENDERER, store::TERMINAL_FONT_SIZE];

const DUPLICATE_SUFFIX: &str = " (Import)";

pub struct ExportOptions {
    pub include_secrets: bool,
    pub include_key_files: bool,
    pub passphrase: Option<SecretString>,
    /// Home directory a leading `~/` in a key path is expanded against.
    pub home: PathBuf,
}

/// What an export left out without failing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// Key paths (as configured) whose files were missing or unreadable and are not embedded.
    pub skipped_keys: Vec<String>,
}

/// The serialized export plus its report.
#[derive(Debug)]
pub struct ExportOutput {
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Host,
    Profile,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    SameId,
    SameName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub id: Id,
    pub name: String,
    pub kind: ItemKind,
    pub conflict: Option<Conflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub items: Vec<ImportItem>,
    pub has_secrets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    Skip,
    Replace,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: u32,
    pub skipped: u32,
    pub secrets_restored: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportDoc {
    format_version: u32,
    exported_at: i64,
    groups: Vec<Group>,
    profiles: Vec<LoginProfile>,
    hosts: Vec<Host>,
    settings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secrets: Option<String>,
}

/// Plaintext of the encrypted block.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretPayload {
    /// keyring account name -> secret value
    secrets: BTreeMap<String, String>,
    /// original key path -> base64 file content
    #[serde(default)]
    key_files: BTreeMap<String, String>,
}

/// Serializes the whole store. Files containing secrets must be written with mode 0600.
pub fn export_data(store: &Store, secrets: &dyn SecretStore, opts: &ExportOptions) -> Result<ExportOutput> {
    if opts.include_key_files && !opts.include_secrets {
        return Err(CoreError::config("key files can only be exported together with secrets"));
    }
    let groups = store.list_groups()?;
    let profiles = store.list_profiles()?;
    let hosts = store.list_hosts()?;
    let mut settings = BTreeMap::new();
    for key in EXPORTED_SETTINGS {
        if let Some(value) = store.get_setting(key)? {
            settings.insert(key.to_string(), value);
        }
    }

    let mut report = ExportReport::default();
    let secrets_block = if opts.include_secrets {
        let passphrase = opts
            .passphrase
            .as_ref()
            .filter(|p| !p.expose_secret().is_empty())
            .ok_or_else(|| CoreError::config("passphrase required to export secrets"))?;
        let payload =
            collect_secrets(&hosts, &profiles, secrets, opts.include_key_files.then_some(&opts.home), &mut report)?;
        let json = serde_json::to_vec(&payload).map_err(|e| CoreError::internal(e.to_string()))?;
        Some(encrypt(&json, passphrase)?)
    } else {
        None
    };

    let doc = ExportDoc {
        format_version: FORMAT_VERSION,
        exported_at: now_unix(),
        groups,
        profiles,
        hosts,
        settings,
        secrets: secrets_block,
    };
    let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| CoreError::internal(e.to_string()))?;
    Ok(ExportOutput { bytes, report })
}

pub fn import_preview(bytes: &[u8], store: &Store) -> Result<ImportPreview> {
    let doc = parse_doc(bytes)?;
    let existing = Existing::load(store)?;
    let mut items = Vec::new();
    for g in &doc.groups {
        items.push(item(g.id, &g.name, ItemKind::Group, existing.conflict(ItemKind::Group, g.id, &g.name)));
    }
    for p in &doc.profiles {
        items.push(item(p.id, &p.name, ItemKind::Profile, existing.conflict(ItemKind::Profile, p.id, &p.name)));
    }
    for h in &doc.hosts {
        items.push(item(h.id, &h.name, ItemKind::Host, existing.conflict(ItemKind::Host, h.id, &h.name)));
    }
    Ok(ImportPreview { items, has_secrets: doc.secrets.is_some() })
}

/// Applies an export file. Conflicting items without an entry in `resolutions` are skipped.
/// Secrets are restored only when the file has a secrets block and `passphrase` is given.
pub fn import_apply(
    bytes: &[u8],
    passphrase: Option<&SecretString>,
    resolutions: &HashMap<Id, Resolution>,
    store: &Store,
    secrets: &dyn SecretStore,
    key_dir: &Path,
) -> Result<ImportReport> {
    let mut doc = parse_doc(bytes)?;
    let payload = match (&doc.secrets, passphrase) {
        (Some(block), Some(pass)) => Some(decrypt_payload(block, pass)?),
        _ => None,
    };

    let mut report = ImportReport { imported: 0, skipped: 0, secrets_restored: 0 };
    match &payload {
        Some(payload) => {
            let paths = write_key_files(&payload.key_files, key_dir)?;
            rewrite_key_paths(&mut doc, &paths);
        }
        None => clear_has_secret(&mut doc),
    }

    let existing = Existing::load(store)?;
    let mut plan = Plan::default();
    for g in &doc.groups {
        plan.decide(&existing, resolutions, ItemKind::Group, g.id, &g.name);
    }
    for p in &doc.profiles {
        plan.decide(&existing, resolutions, ItemKind::Profile, p.id, &p.name);
    }
    for h in &doc.hosts {
        plan.decide(&existing, resolutions, ItemKind::Host, h.id, &h.name);
    }

    for g in &doc.groups {
        match plan.action(g.id) {
            Action::Write { id, name } => {
                store.save_group(&Group { id, name, sort_order: g.sort_order })?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }
    for p in &doc.profiles {
        match plan.action(p.id) {
            Action::Write { id, name } => {
                store.save_profile(&LoginProfile { id, name, ..p.clone() })?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }
    for h in &doc.hosts {
        match plan.action(h.id) {
            Action::Write { id, name } => {
                let mut host = h.clone();
                if id != h.id {
                    for fwd in &mut host.forwards {
                        fwd.id = Id::new_v4();
                    }
                }
                host.id = id;
                host.name = name;
                host.group_id = h.group_id.map(|g| plan.map_id(g));
                host.jump_host_id = h.jump_host_id.map(|j| plan.map_id(j));
                if let HostAuth::Profile { profile_id } = &mut host.auth {
                    *profile_id = plan.map_id(*profile_id);
                }
                store.save_host(&host)?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }

    if let Some(payload) = &payload {
        for (account, value) in &payload.secrets {
            let Some((owner, kind)) = parse_account_name(account) else { continue };
            let Action::Write { id, .. } = plan.action(owner.id()) else { continue };
            let owner = match owner {
                SecretOwner::Host(_) => SecretOwner::Host(id),
                SecretOwner::Profile(_) => SecretOwner::Profile(id),
            };
            secrets.set(owner, kind, &SecretString::from(value.clone()))?;
            report.secrets_restored += 1;
        }
    }

    for (key, value) in &doc.settings {
        if EXPORTED_SETTINGS.contains(&key.as_str()) {
            store.set_setting(key, value)?;
        }
    }
    Ok(report)
}

fn item(id: Id, name: &str, kind: ItemKind, conflict: Option<Conflict>) -> ImportItem {
    ImportItem { id, name: name.to_string(), kind, conflict }
}

fn parse_doc(bytes: &[u8]) -> Result<ExportDoc> {
    let doc: ExportDoc =
        serde_json::from_slice(bytes).map_err(|e| CoreError::config(format!("invalid export file: {e}")))?;
    if doc.format_version > FORMAT_VERSION {
        return Err(CoreError::config(format!("unsupported export format version {}", doc.format_version)));
    }
    Ok(doc)
}

/// Ids and names of what is already stored, per item kind.
struct Existing {
    items: Vec<(ItemKind, Id, String)>,
}

impl Existing {
    fn load(store: &Store) -> Result<Self> {
        let mut items = Vec::new();
        items.extend(store.list_groups()?.into_iter().map(|g| (ItemKind::Group, g.id, g.name)));
        items.extend(store.list_profiles()?.into_iter().map(|p| (ItemKind::Profile, p.id, p.name)));
        items.extend(store.list_hosts()?.into_iter().map(|h| (ItemKind::Host, h.id, h.name)));
        Ok(Self { items })
    }

    fn conflict(&self, kind: ItemKind, id: Id, name: &str) -> Option<Conflict> {
        if self.items.iter().any(|(k, i, _)| *k == kind && *i == id) {
            Some(Conflict::SameId)
        } else if self.same_name(kind, name).is_some() {
            Some(Conflict::SameName)
        } else {
            None
        }
    }

    fn same_name(&self, kind: ItemKind, name: &str) -> Option<Id> {
        self.items.iter().find(|(k, _, n)| *k == kind && n == name).map(|(_, id, _)| *id)
    }
}

#[derive(Clone)]
enum Action {
    Write { id: Id, name: String },
    Skip,
}

/// Per imported item: what to do, and which stored id references to it should point at.
#[derive(Default)]
struct Plan {
    actions: HashMap<Id, Action>,
    id_map: HashMap<Id, Id>,
}

impl Plan {
    fn decide(
        &mut self,
        existing: &Existing,
        resolutions: &HashMap<Id, Resolution>,
        kind: ItemKind,
        id: Id,
        name: &str,
    ) {
        let conflict = existing.conflict(kind, id, name);
        let resolution = resolutions.get(&id).copied().unwrap_or(Resolution::Skip);
        // For a same-name conflict, "replace" and "skip" both resolve references to the stored item.
        let target = match conflict {
            Some(Conflict::SameName) => existing.same_name(kind, name).unwrap_or(id),
            _ => id,
        };
        let (action, mapped) = match (conflict, resolution) {
            (None, _) => (Action::Write { id, name: name.to_string() }, id),
            (Some(_), Resolution::Skip) => (Action::Skip, target),
            (Some(_), Resolution::Replace) => (Action::Write { id: target, name: name.to_string() }, target),
            (Some(_), Resolution::Duplicate) => {
                let new_id = Id::new_v4();
                (Action::Write { id: new_id, name: format!("{name}{DUPLICATE_SUFFIX}") }, new_id)
            }
        };
        self.actions.insert(id, action);
        self.id_map.insert(id, mapped);
    }

    fn action(&self, id: Id) -> Action {
        self.actions.get(&id).cloned().unwrap_or(Action::Skip)
    }

    /// Ids not part of the import (e.g. ssh-config jump hosts) stay unchanged.
    fn map_id(&self, id: Id) -> Id {
        self.id_map.get(&id).copied().unwrap_or(id)
    }
}

fn collect_secrets(
    hosts: &[Host],
    profiles: &[LoginProfile],
    secrets: &dyn SecretStore,
    key_files_home: Option<&PathBuf>,
    report: &mut ExportReport,
) -> Result<SecretPayload> {
    let mut payload = SecretPayload::default();
    let owners =
        hosts.iter().map(|h| SecretOwner::Host(h.id)).chain(profiles.iter().map(|p| SecretOwner::Profile(p.id)));
    for owner in owners {
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            if let Some(value) = secrets.get(owner, kind)? {
                payload.secrets.insert(account_name(owner, kind), value.expose_secret().to_string());
            }
        }
    }
    if let Some(home) = key_files_home {
        let auths = hosts
            .iter()
            .filter_map(|h| match &h.auth {
                HostAuth::Custom { auth } => Some(auth),
                HostAuth::Profile { .. } => None,
            })
            .chain(profiles.iter().map(|p| &p.auth));
        for auth in auths {
            let Some(path) = key_path(auth) else { continue };
            if payload.key_files.contains_key(path) || report.skipped_keys.iter().any(|p| p == path) {
                continue;
            }
            // Keyed by the configured path: import rewrites exactly that value.
            match fs::read(expand_tilde(path, home)) {
                Ok(content) => {
                    payload.key_files.insert(path.to_string(), B64.encode(content));
                }
                // Missing or unreadable keys are reported, not fatal: the rest of the export stays useful.
                Err(err) => {
                    tracing::warn!(kind = ?err.kind(), "key file not exported");
                    report.skipped_keys.push(path.to_string());
                }
            }
        }
    }
    Ok(payload)
}

fn key_path(auth: &AuthConfig) -> Option<&str> {
    match auth {
        AuthConfig { method: AuthMethod::Key, key_path: Some(path), .. } if !path.is_empty() => Some(path),
        _ => None,
    }
}

/// Writes key files into `key_dir` (0700) as 0600 files and returns original -> new path.
fn write_key_files(files: &BTreeMap<String, String>, key_dir: &Path) -> Result<HashMap<String, String>> {
    let mut mapping = HashMap::new();
    if files.is_empty() {
        return Ok(mapping);
    }
    fsutil::create_private_dir(key_dir)?;
    for (original, encoded) in files {
        let content = B64.decode(encoded).map_err(|e| CoreError::config(format!("invalid key file in export: {e}")))?;
        let base = Path::new(original)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "key".to_string());
        let target = unique_key_path(key_dir, &base, &content)?;
        fsutil::write_private(&target, &content)?;
        mapping.insert(original.clone(), target.to_string_lossy().into_owned());
    }
    Ok(mapping)
}

/// `base`, `base-1`, `base-2`, ...; an existing file with identical content is reused.
fn unique_key_path(dir: &Path, base: &str, content: &[u8]) -> Result<PathBuf> {
    for n in 0u32.. {
        let name = if n == 0 { base.to_string() } else { format!("{base}-{n}") };
        let candidate = dir.join(name);
        match fs::read(&candidate) {
            Ok(existing) if existing == content => return Ok(candidate),
            Ok(_) => continue,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(candidate),
            Err(err) => return Err(err.into()),
        }
    }
    unreachable!("u32 range exhausted while choosing a key file name")
}

fn rewrite_key_paths(doc: &mut ExportDoc, paths: &HashMap<String, String>) {
    let rewrite = |auth: &mut AuthConfig| {
        if let Some(new_path) = auth.key_path.as_ref().and_then(|p| paths.get(p)) {
            auth.key_path = Some(new_path.clone());
        }
    };
    for host in &mut doc.hosts {
        if let HostAuth::Custom { auth } = &mut host.auth {
            rewrite(auth);
        }
    }
    for profile in &mut doc.profiles {
        rewrite(&mut profile.auth);
    }
}

/// Without restored secrets nothing is in the keyring, so `has_secret` must not claim otherwise.
fn clear_has_secret(doc: &mut ExportDoc) {
    for host in &mut doc.hosts {
        if let HostAuth::Custom { auth } = &mut host.auth {
            auth.has_secret = false;
        }
    }
    for profile in &mut doc.profiles {
        profile.auth.has_secret = false;
    }
}

fn encrypt(plaintext: &[u8], passphrase: &SecretString) -> Result<String> {
    let mut recipient = age::scrypt::Recipient::new(passphrase.clone());
    recipient.set_work_factor(SCRYPT_WORK_FACTOR);
    let encryptor = age::Encryptor::with_recipients(iter::once(&recipient as &dyn age::Recipient))
        .map_err(|e| CoreError::internal(e.to_string()))?;
    let mut armored = Vec::new();
    let armor = ArmoredWriter::wrap_output(&mut armored, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor)?;
    writer.write_all(plaintext)?;
    writer.finish()?.finish()?;
    String::from_utf8(armored).map_err(|e| CoreError::internal(e.to_string()))
}

/// `ExcessiveWork` means the stanza's work factor exceeds what we accept (either a hostile
/// file or, in principle, a future higher `SCRYPT_WORK_FACTOR`); anything else means the
/// passphrase (or file) was wrong.
///
/// We build our own message from `required`/`target` instead of formatting `e` directly:
/// `age`'s own `Display` for `ExcessiveWork` computes `1 << (required - target)` and panics
/// on overflow in debug builds whenever `target` (this machine's own calibrated work
/// factor) happens to exceed `required` (the file's work factor) — exactly the case a fast
/// machine hits when checking an import encrypted at our fixed, lower `SCRYPT_WORK_FACTOR`.
fn map_decrypt_error(e: age::DecryptError) -> CoreError {
    match e {
        age::DecryptError::ExcessiveWork { required, .. } => CoreError::config(format!(
            "export file cannot be decrypted on this machine: it was encrypted with scrypt \
             work factor 2^{required}, above the maximum this machine accepts"
        )),
        _ => CoreError::config("wrong passphrase"),
    }
}

fn decrypt_payload(block: &str, passphrase: &SecretString) -> Result<SecretPayload> {
    let decryptor = age::Decryptor::new(ArmoredReader::new(block.as_bytes()))
        .map_err(|e| CoreError::config(format!("invalid secrets block: {e}")))?;
    let mut identity = age::scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(SCRYPT_MAX_WORK_FACTOR);
    let mut reader = decryptor.decrypt(iter::once(&identity as &dyn age::Identity)).map_err(map_decrypt_error)?;
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    serde_json::from_slice(&plaintext).map_err(|e| CoreError::config(format!("invalid secrets block: {e}")))
}

#[cfg(test)]
mod tests;
