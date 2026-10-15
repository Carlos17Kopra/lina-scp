use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::model::{ForwardKind, PortForward};
use crate::secrets::MemorySecretStore;

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

fn no_secrets() -> ExportOptions {
    ExportOptions {
        include_secrets: false,
        include_key_files: false,
        passphrase: None,
        home: PathBuf::from("/nonexistent"),
    }
}

fn with_secrets(passphrase: &str, key_files: bool) -> ExportOptions {
    ExportOptions {
        include_secrets: true,
        include_key_files: key_files,
        passphrase: Some(pass(passphrase)),
        home: PathBuf::from("/nonexistent"),
    }
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
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap().bytes;
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
    assert_eq!(report, ImportReport { imported: 3, skipped: 0, secrets_restored: 0, invalid: 0 });

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
    let opts =
        ExportOptions { include_secrets: true, include_key_files: false, passphrase: None, home: PathBuf::new() };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
    let opts =
        ExportOptions { include_secrets: false, include_key_files: true, passphrase: None, home: PathBuf::new() };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
}

#[test]
fn roundtrip_with_secrets_and_key_files() {
    let tmp = tempfile::tempdir().unwrap();
    let key_src = tmp.path().join("id_test");
    fs::write(&key_src, b"PRIVATE KEY DATA").unwrap();
    let src = source(Some(&key_src));

    let bytes = export_data(&src.store, &src.secrets, &with_secrets("correct horse", true)).unwrap().bytes;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("-----BEGIN AGE ENCRYPTED FILE-----"));
    assert!(!text.contains("s3cret") && !text.contains("PRIVATE KEY DATA"));

    let dst = Store::open_in_memory().unwrap();
    let dst_secrets = MemorySecretStore::new();
    assert!(import_preview(&bytes, &dst).unwrap().has_secrets);
    let key_dir = tmp.path().join("imported");
    let report =
        import_apply(&bytes, Some(&pass("correct horse")), &HashMap::new(), &dst, &dst_secrets, &key_dir).unwrap();
    assert_eq!(report, ImportReport { imported: 4, skipped: 0, secrets_restored: 2, invalid: 0 });

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
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("right", false)).unwrap().bytes;
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
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap().bytes;
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
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap().bytes;
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &HashMap::new(), &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 0, skipped: 3, secrets_restored: 0, invalid: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "changed");
}

#[test]
fn replace_same_id_overwrites() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap().bytes;
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let resolutions = HashMap::from([(src.host.id, Resolution::Replace)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 1, skipped: 2, secrets_restored: 0, invalid: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "10.0.0.5");
}

#[test]
fn replace_same_name_keeps_existing_id() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap().bytes;
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
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("pw", false)).unwrap().bytes;
    let resolutions = HashMap::from([(src.profile.id, Resolution::Duplicate), (src.host.id, Resolution::Duplicate)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, Some(&pass("pw")), &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 2, skipped: 1, secrets_restored: 1, invalid: 0 });
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

/// `encrypt` must use a fixed scrypt work factor (not one calibrated to this machine's
/// speed), so a file produced on a fast machine still decrypts on a slow one. We prove it
/// by round-tripping, then showing an identity capped at exactly `SCRYPT_WORK_FACTOR`
/// accepts the stanza while one capped one step lower refuses it as `ExcessiveWork`, which
/// `decrypt_payload` maps to the "cannot be decrypted on this machine" config error.
#[test]
fn encrypt_uses_fixed_work_factor_decryptable_regardless_of_machine_speed() {
    let passphrase = pass("whatever");
    let block = encrypt(b"plaintext payload", &passphrase).unwrap();

    let mut identity = age::scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(SCRYPT_WORK_FACTOR);
    let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(block.as_bytes())).unwrap();
    let mut reader = decryptor.decrypt(iter::once(&identity as &dyn age::Identity)).unwrap();
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext).unwrap();
    assert_eq!(plaintext, b"plaintext payload");

    let mut too_strict = age::scrypt::Identity::new(passphrase);
    too_strict.set_max_work_factor(SCRYPT_WORK_FACTOR - 1);
    let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(block.as_bytes())).unwrap();
    let Err(err) = decryptor.decrypt(iter::once(&too_strict as &dyn age::Identity)) else {
        panic!("expected decryption to be refused for exceeding the work factor cap");
    };
    assert!(matches!(err, age::DecryptError::ExcessiveWork { required, .. } if required == SCRYPT_WORK_FACTOR));
    assert!(matches!(map_decrypt_error(err), CoreError::Config { .. }));
}

/// `decrypt_payload` must accept a file produced with our fixed work factor regardless of
/// how fast this machine is, i.e. `SCRYPT_MAX_WORK_FACTOR >= SCRYPT_WORK_FACTOR`.
#[test]
fn decrypt_payload_accepts_the_fixed_work_factor() {
    let passphrase = pass("whatever");
    let payload = SecretPayload { secrets: BTreeMap::from([("a".to_string(), "b".to_string())]), ..Default::default() };
    let json = serde_json::to_vec(&payload).unwrap();
    let block = encrypt(&json, &passphrase).unwrap();
    let decoded = decrypt_payload(&block, &passphrase).unwrap();
    assert_eq!(decoded.secrets.get("a").map(String::as_str), Some("b"));
}

#[test]
fn key_file_with_tilde_path_is_expanded_against_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".ssh")).unwrap();
    fs::write(home.join(".ssh/id_tilde"), b"TILDE KEY DATA").unwrap();
    let src = source(Some(Path::new("~/.ssh/id_tilde")));

    let mut opts = with_secrets("correct horse", true);
    opts.home = home;
    let output = export_data(&src.store, &src.secrets, &opts).unwrap();
    assert!(output.report.skipped_keys.is_empty(), "nothing skipped: {:?}", output.report.skipped_keys);

    let dst = Store::open_in_memory().unwrap();
    let key_dir = tmp.path().join("imported");
    import_apply(
        &output.bytes,
        Some(&pass("correct horse")),
        &HashMap::new(),
        &dst,
        &MemorySecretStore::new(),
        &key_dir,
    )
    .unwrap();
    let key_host = dst.list_hosts().unwrap().into_iter().find(|h| h.name == "keyhost").unwrap();
    let HostAuth::Custom { auth } = &key_host.auth else { panic!("expected custom auth") };
    let new_path = PathBuf::from(auth.key_path.clone().unwrap());
    assert_eq!(new_path, key_dir.join("id_tilde"));
    assert_eq!(fs::read(&new_path).unwrap(), b"TILDE KEY DATA");
}

#[test]
fn missing_key_file_is_reported_and_does_not_fail_the_export() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("id_gone");
    let src = source(Some(&missing));

    let output = export_data(&src.store, &src.secrets, &with_secrets("correct horse", true)).unwrap();
    assert_eq!(output.report.skipped_keys, vec![missing.to_string_lossy().into_owned()]);

    // The rest of the export (entries and secrets) is still complete.
    let dst = Store::open_in_memory().unwrap();
    let report = import_apply(
        &output.bytes,
        Some(&pass("correct horse")),
        &HashMap::new(),
        &dst,
        &MemorySecretStore::new(),
        tmp.path(),
    )
    .unwrap();
    assert_eq!(report.imported, 4);
    assert_eq!(report.secrets_restored, 2);
}

#[test]
fn unreadable_key_file_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    // A directory cannot be read as a file: stands in for any read error (permissions are
    // not reliable when the tests run as root).
    let unreadable = tmp.path().join("id_dir");
    fs::create_dir(&unreadable).unwrap();
    let src = source(Some(&unreadable));

    let output = export_data(&src.store, &src.secrets, &with_secrets("correct horse", true)).unwrap();
    assert_eq!(output.report.skipped_keys, vec![unreadable.to_string_lossy().into_owned()]);
}

/// F5: a crafted export file must not smuggle ssh_config directives into the mirror file.
#[test]
fn import_rejects_hosts_that_would_inject_ssh_config_directives() {
    let src = Store::open_in_memory().unwrap();
    let good = Host::new("good", "good.example");
    let mut evil = Host::new("evil", "x.example\n    ProxyCommand touch /tmp/pwned");
    evil.user = Some("root".into());
    let profile = LoginProfile::new(
        "evil-profile",
        "ops\"\nMatch exec \"touch /tmp/pwned",
        AuthConfig { method: AuthMethod::Password, key_path: None, has_secret: false },
    );
    let mut via_profile = Host::new("via-profile", "p.example");
    via_profile.auth = HostAuth::Profile { profile_id: profile.id };
    src.save_host(&good).unwrap();
    src.save_host(&evil).unwrap();
    src.save_profile(&profile).unwrap();
    src.save_host(&via_profile).unwrap();
    let bytes = export_data(&src, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes;

    let dst = Store::open_in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let report = import_apply(&bytes, None, &HashMap::new(), &dst, &MemorySecretStore::new(), tmp.path()).unwrap();

    assert_eq!(report.imported, 1, "only the valid host is written: {report:?}");
    assert_eq!(report.invalid, 3, "evil host, evil profile and the host using it: {report:?}");
    assert!(dst.get_host(good.id).unwrap().is_some());
    assert!(dst.get_host(evil.id).unwrap().is_none());
    assert!(dst.get_profile(profile.id).unwrap().is_none());
    assert!(dst.get_host(via_profile.id).unwrap().is_none(), "a host using a rejected profile is rejected too");

    let ssh_dir = tmp.path().join(".ssh");
    let mirror = crate::sshconfig::Mirror::new(ssh_dir.clone());
    mirror.enable(&dst.list_hosts().unwrap(), &dst.list_profiles().unwrap()).unwrap();
    let conf = fs::read_to_string(ssh_dir.join("config.d/linascp.conf")).unwrap();
    assert!(conf.contains("Host good\n"), "{conf}");
    assert!(!conf.contains("ProxyCommand") && !conf.contains("Match") && !conf.contains("pwned"), "{conf}");
}

fn key_host(name: &str, address: &str, key_path: &str) -> Host {
    let mut host = Host::new(name, address);
    host.auth = HostAuth::Custom {
        auth: AuthConfig { method: AuthMethod::Key, key_path: Some(key_path.into()), has_secret: false },
    };
    host
}

/// An export file with the given hosts and embedded key files (path -> content), passphrase "pw".
fn doc_with_keys(hosts: Vec<Host>, key_files: &[(&str, &[u8])]) -> Vec<u8> {
    let payload = SecretPayload {
        secrets: BTreeMap::new(),
        key_files: key_files.iter().map(|(path, content)| (path.to_string(), B64.encode(content))).collect(),
    };
    let doc = ExportDoc {
        format_version: FORMAT_VERSION,
        exported_at: 0,
        groups: vec![Group::new("G", 0)],
        profiles: vec![],
        hosts,
        settings: BTreeMap::new(),
        secrets: Some(encrypt(&serde_json::to_vec(&payload).unwrap(), &pass("pw")).unwrap()),
    };
    serde_json::to_vec(&doc).unwrap()
}

#[test]
fn key_files_of_rejected_or_skipped_hosts_are_not_written() {
    let existing = Host::new("taken", "t.example");
    let dst = Store::open_in_memory().unwrap();
    dst.save_host(&existing).unwrap();
    let bytes = doc_with_keys(
        vec![
            key_host("good", "g.example", "/k/id_good"),
            key_host("evil", "x.example\n    ProxyCommand touch /tmp/pwned", "/k/id_evil"),
            key_host("taken", "t.example", "/k/id_taken"),
        ],
        &[("/k/id_good", b"GOOD"), ("/k/id_evil", b"EVIL"), ("/k/id_taken", b"TAKEN")],
    );
    let tmp = tempfile::tempdir().unwrap();
    let key_dir = tmp.path().join("keys");

    let report =
        import_apply(&bytes, Some(&pass("pw")), &HashMap::new(), &dst, &MemorySecretStore::new(), &key_dir).unwrap();

    assert_eq!((report.invalid, report.skipped), (1, 1), "{report:?}");
    let mut written: Vec<_> = fs::read_dir(&key_dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    written.sort();
    assert_eq!(written, vec!["id_good"]);
}

#[test]
fn rejection_follows_jump_chains_in_any_order() {
    let bad = Host::new("c-bad", "x.example\n    ProxyCommand touch /tmp/pwned");
    let mut middle = Host::new("b-middle", "m.example");
    middle.jump_host_id = Some(bad.id);
    let mut last = Host::new("a-last", "l.example");
    last.jump_host_id = Some(middle.id);
    let src = Store::open_in_memory().unwrap();
    for host in [&bad, &middle, &last] {
        src.save_host(host).unwrap();
    }
    let bytes = export_data(&src, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes;

    let dst = Store::open_in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let report = import_apply(&bytes, None, &HashMap::new(), &dst, &MemorySecretStore::new(), tmp.path()).unwrap();

    assert_eq!((report.imported, report.invalid), (0, 3), "{report:?}");
    assert!(dst.list_hosts().unwrap().is_empty());
}

#[test]
fn failed_import_writes_nothing() {
    // Two hosts sharing a port forward id make the second insert fail half-way.
    let forward = PortForward {
        id: Id::new_v4(),
        kind: ForwardKind::Local,
        bind: "8080".into(),
        target: "localhost:80".into(),
        autostart: false,
    };
    let mut first = key_host("first", "f.example", "/k/id_first");
    first.forwards = vec![forward.clone()];
    let mut second = Host::new("second", "s.example");
    second.forwards = vec![forward];
    let bytes = doc_with_keys(vec![first, second], &[("/k/id_first", b"FIRST")]);
    let dst = Store::open_in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let key_dir = tmp.path().join("keys");

    let result = import_apply(&bytes, Some(&pass("pw")), &HashMap::new(), &dst, &MemorySecretStore::new(), &key_dir);

    assert!(result.is_err());
    assert!(dst.list_groups().unwrap().is_empty() && dst.list_hosts().unwrap().is_empty());
    assert!(fs::read_dir(&key_dir).map(|mut d| d.next().is_none()).unwrap_or(true), "no key file left behind");
}
