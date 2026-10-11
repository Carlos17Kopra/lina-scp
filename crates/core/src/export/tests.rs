use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::model::{ForwardKind, PortForward};
use crate::secrets::MemorySecretStore;

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

fn no_secrets() -> ExportOptions {
    ExportOptions { include_secrets: false, include_key_files: false, passphrase: None }
}

fn with_secrets(passphrase: &str, key_files: bool) -> ExportOptions {
    ExportOptions { include_secrets: true, include_key_files: key_files, passphrase: Some(pass(passphrase)) }
}

struct Source {
    store: Store,
    secrets: MemorySecretStore,
    group: Group,
    profile: LoginProfile,
    host: Host,
}

/// group + profile (password secret) + host (uses group, profile, forward) + key host.
fn source(key_path: Option<&Path>) -> Source {
    let store = Store::open_in_memory().unwrap();
    let secrets = MemorySecretStore::new();
    let group = Group::new("Prod", 0);
    let profile = LoginProfile::new(
        "admin",
        "root",
        AuthConfig { method: AuthMethod::Password, key_path: None, has_secret: true },
    );
    let mut host = Host::new("web", "10.0.0.5");
    host.group_id = Some(group.id);
    host.auth = HostAuth::Profile { profile_id: profile.id };
    host.forwards = vec![PortForward {
        id: Id::new_v4(),
        kind: ForwardKind::Local,
        bind: "8080".into(),
        target: "localhost:80".into(),
        autostart: false,
    }];
    store.save_group(&group).unwrap();
    store.save_profile(&profile).unwrap();
    store.save_host(&host).unwrap();
    secrets.set(SecretOwner::Profile(profile.id), SecretKind::Password, &pass("s3cret")).unwrap();
    if let Some(path) = key_path {
        let mut key_host = Host::new("keyhost", "10.0.0.6");
        key_host.auth = HostAuth::Custom {
            auth: AuthConfig {
                method: AuthMethod::Key,
                key_path: Some(path.to_string_lossy().into_owned()),
                has_secret: true,
            },
        };
        store.save_host(&key_host).unwrap();
        secrets.set(SecretOwner::Host(key_host.id), SecretKind::Passphrase, &pass("keypass")).unwrap();
    }
    store.set_setting(store::THEME, "light").unwrap();
    store.set_setting(store::SSH_MODE, "mirror").unwrap();
    Source { store, secrets, group, profile, host }
}

#[test]
fn roundtrip_without_secrets_into_empty_store() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["formatVersion"], 1);
    assert!(json.get("secrets").is_none());
    assert!(!String::from_utf8_lossy(&bytes).contains("s3cret"));

    let dst = Store::open_in_memory().unwrap();
    let dst_secrets = MemorySecretStore::new();
    let preview = import_preview(&bytes, &dst).unwrap();
    assert!(!preview.has_secrets);
    assert_eq!(preview.items.len(), 3);
    assert!(preview.items.iter().all(|i| i.conflict.is_none()));

    let tmp = tempfile::tempdir().unwrap();
    let report = import_apply(&bytes, None, &HashMap::new(), &dst, &dst_secrets, tmp.path()).unwrap();
    assert_eq!(report, ImportReport { imported: 3, skipped: 0, secrets_restored: 0 });

    let host = dst.get_host(src.host.id).unwrap().unwrap();
    assert_eq!(host.group_id, Some(src.group.id));
    assert_eq!(host.auth, HostAuth::Profile { profile_id: src.profile.id });
    assert_eq!(host.forwards, src.host.forwards);
    let profile = dst.get_profile(src.profile.id).unwrap().unwrap();
    assert!(!profile.auth.has_secret, "has_secret cleared when no secrets were restored");
    assert_eq!(dst.get_setting(store::THEME).unwrap().as_deref(), Some("light"));
    assert_eq!(dst.get_setting(store::SSH_MODE).unwrap(), None);
    assert!(dst_secrets.is_empty());
}

#[test]
fn export_with_secrets_requires_passphrase() {
    let src = source(None);
    let opts = ExportOptions { include_secrets: true, include_key_files: false, passphrase: None };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
    let opts = ExportOptions { include_secrets: false, include_key_files: true, passphrase: None };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
}

#[test]
fn roundtrip_with_secrets_and_key_files() {
    let tmp = tempfile::tempdir().unwrap();
    let key_src = tmp.path().join("id_test");
    fs::write(&key_src, b"PRIVATE KEY DATA").unwrap();
    let src = source(Some(&key_src));

    let bytes = export_data(&src.store, &src.secrets, &with_secrets("correct horse", true)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("-----BEGIN AGE ENCRYPTED FILE-----"));
    assert!(!text.contains("s3cret") && !text.contains("PRIVATE KEY DATA"));

    let dst = Store::open_in_memory().unwrap();
    let dst_secrets = MemorySecretStore::new();
    assert!(import_preview(&bytes, &dst).unwrap().has_secrets);
    let key_dir = tmp.path().join("imported");
    let report =
        import_apply(&bytes, Some(&pass("correct horse")), &HashMap::new(), &dst, &dst_secrets, &key_dir).unwrap();
    assert_eq!(report, ImportReport { imported: 4, skipped: 0, secrets_restored: 2 });

    let restored = dst_secrets.get(SecretOwner::Profile(src.profile.id), SecretKind::Password).unwrap().unwrap();
    assert_eq!(restored.expose_secret(), "s3cret");

    let key_host = dst.list_hosts().unwrap().into_iter().find(|h| h.name == "keyhost").unwrap();
    let HostAuth::Custom { auth } = &key_host.auth else { panic!("expected custom auth") };
    let new_path = PathBuf::from(auth.key_path.clone().unwrap());
    assert_eq!(new_path, key_dir.join("id_test"));
    assert_eq!(fs::read(&new_path).unwrap(), b"PRIVATE KEY DATA");
    assert_eq!(fs::metadata(&new_path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(auth.has_secret);
    let passphrase = dst_secrets.get(SecretOwner::Host(key_host.id), SecretKind::Passphrase).unwrap().unwrap();
    assert_eq!(passphrase.expose_secret(), "keypass");
}

#[test]
fn wrong_passphrase_is_config_error_and_imports_nothing() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("right", false)).unwrap();
    let dst = Store::open_in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let err = import_apply(&bytes, Some(&pass("wrong")), &HashMap::new(), &dst, &MemorySecretStore::new(), tmp.path())
        .unwrap_err();
    assert_eq!(err, CoreError::config("wrong passphrase"));
    assert!(dst.list_hosts().unwrap().is_empty());
}

#[test]
fn invalid_file_is_config_error() {
    let dst = Store::open_in_memory().unwrap();
    assert!(matches!(import_preview(b"not json", &dst), Err(CoreError::Config { .. })));
    let future = br#"{"formatVersion":99,"exportedAt":0,"groups":[],"profiles":[],"hosts":[],"settings":{}}"#;
    assert!(matches!(import_preview(future, &dst), Err(CoreError::Config { .. })));
}

#[test]
fn preview_detects_same_id_before_same_name() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    // Importing into the source store: every item conflicts by id.
    let preview = import_preview(&bytes, &src.store).unwrap();
    assert!(preview.items.iter().all(|i| i.conflict == Some(Conflict::SameId)));

    let other = Store::open_in_memory().unwrap();
    other.save_host(&Host::new("web", "elsewhere")).unwrap();
    let preview = import_preview(&bytes, &other).unwrap();
    let web = preview.items.iter().find(|i| i.kind == ItemKind::Host).unwrap();
    assert_eq!(web.conflict, Some(Conflict::SameName));
    let group = preview.items.iter().find(|i| i.kind == ItemKind::Group).unwrap();
    assert_eq!(group.conflict, None);
}

#[test]
fn conflicts_default_to_skip() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &HashMap::new(), &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 0, skipped: 3, secrets_restored: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "changed");
}

#[test]
fn replace_same_id_overwrites() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let resolutions = HashMap::from([(src.host.id, Resolution::Replace)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 1, skipped: 2, secrets_restored: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "10.0.0.5");
}

#[test]
fn replace_same_name_keeps_existing_id() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let dst = Store::open_in_memory().unwrap();
    let existing = Host::new("web", "elsewhere");
    dst.save_host(&existing).unwrap();
    let resolutions = HashMap::from([(src.host.id, Resolution::Replace)]);
    let tmp = tempfile::tempdir().unwrap();

    import_apply(&bytes, None, &resolutions, &dst, &MemorySecretStore::new(), tmp.path()).unwrap();

    let hosts = dst.list_hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].id, existing.id);
    assert_eq!(hosts[0].address, "10.0.0.5");
    assert_eq!(hosts[0].auth, HostAuth::Profile { profile_id: src.profile.id });
}

#[test]
fn duplicate_gets_new_ids_suffix_and_remapped_secrets() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("pw", false)).unwrap();
    let resolutions = HashMap::from([(src.profile.id, Resolution::Duplicate), (src.host.id, Resolution::Duplicate)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, Some(&pass("pw")), &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 2, skipped: 1, secrets_restored: 1 });
    let profiles = src.store.list_profiles().unwrap();
    let dup_profile = profiles.iter().find(|p| p.name == "admin (Import)").unwrap();
    assert_ne!(dup_profile.id, src.profile.id);
    let hosts = src.store.list_hosts().unwrap();
    let dup_host = hosts.iter().find(|h| h.name == "web (Import)").unwrap();
    assert_ne!(dup_host.id, src.host.id);
    assert_eq!(dup_host.auth, HostAuth::Profile { profile_id: dup_profile.id });
    assert_eq!(dup_host.group_id, Some(src.group.id), "skipped group resolves to the stored one");
    assert_ne!(dup_host.forwards[0].id, src.host.forwards[0].id);
    let secret = src.secrets.get(SecretOwner::Profile(dup_profile.id), SecretKind::Password).unwrap().unwrap();
    assert_eq!(secret.expose_secret(), "s3cret");
}
