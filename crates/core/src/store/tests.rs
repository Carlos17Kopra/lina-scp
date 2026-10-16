use super::*;
use crate::model::{EnvVar, SftpSettings, Snippet};

fn full_host() -> Host {
    let mut h = Host::new("web-1", "10.0.0.5");
    h.port = 2222;
    h.user = Some("deploy".into());
    h.color = Some("#ff8800".into());
    h.notes = "production".into();
    h.tags = vec!["prod".into(), "eu".into()];
    h.protocol_sftp = false;
    h.auth = HostAuth::Custom {
        auth: AuthConfig {
            method: AuthMethod::Key,
            key_path: Some("/home/u/.ssh/id_ed25519".into()),
            has_secret: true,
        },
    };
    h.keepalive_s = 10;
    h.timeout_s = 5;
    h.start_command = Some("tmux attach".into());
    h.terminal_scheme = Some("nord".into());
    h.terminal_font_size = Some(16);
    h.env = vec![EnvVar { key: "LANG".into(), value: "C.UTF-8".into() }];
    h.sftp = SftpSettings {
        remote_start: Some("/var/www".into()),
        local_start: None,
        parallel: 8,
        upload_perms: Some("0644".into()),
        bandwidth_limit_kbps: Some(512),
        sftp_server_cmd: Some("sudo /usr/lib/openssh/sftp-server".into()),
    };
    h.forwards = vec![
        PortForward {
            id: uuid::Uuid::new_v4(),
            kind: ForwardKind::Local,
            bind: "127.0.0.1:8080".into(),
            target: "localhost:80".into(),
            autostart: true,
        },
        PortForward {
            id: uuid::Uuid::new_v4(),
            kind: ForwardKind::Dynamic,
            bind: "1080".into(),
            target: String::new(),
            autostart: false,
        },
    ];
    h
}

#[test]
fn migrations_are_idempotent_and_set_user_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sub/linascp.db");
    Store::open(&path).unwrap();
    let store = Store::open(&path).unwrap();
    let version: i64 = store.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, migrations::LATEST_VERSION);
}

#[test]
fn host_roundtrip_with_all_fields() {
    let store = Store::open_in_memory().unwrap();
    let group = Group::new("Prod", 1);
    store.save_group(&group).unwrap();
    let mut host = full_host();
    host.group_id = Some(group.id);
    host.created_at = 1_000;
    host.updated_at = 1_000;
    store.save_host(&host).unwrap();

    let loaded = store.get_host(host.id).unwrap().unwrap();
    assert!(loaded.updated_at >= now_unix() - 5);
    let mut expected = host.clone();
    expected.updated_at = loaded.updated_at;
    assert_eq!(loaded, expected);
    assert_eq!(store.list_hosts().unwrap(), vec![expected]);
}

#[test]
fn save_host_upserts_and_replaces_children() {
    let store = Store::open_in_memory().unwrap();
    let mut host = full_host();
    store.save_host(&host).unwrap();
    host.name = "web-renamed".into();
    host.tags = vec!["staging".into()];
    host.forwards.truncate(1);
    store.save_host(&host).unwrap();

    let loaded = store.get_host(host.id).unwrap().unwrap();
    assert_eq!(loaded.name, "web-renamed");
    assert_eq!(loaded.tags, vec!["staging".to_string()]);
    assert_eq!(loaded.forwards, host.forwards);
    assert_eq!(store.list_hosts().unwrap().len(), 1);
}

#[test]
fn list_hosts_sorted_by_name_case_insensitive() {
    let store = Store::open_in_memory().unwrap();
    for name in ["beta", "Alpha", "gamma"] {
        store.save_host(&Host::new(name, "h")).unwrap();
    }
    let names: Vec<String> = store.list_hosts().unwrap().into_iter().map(|h| h.name).collect();
    assert_eq!(names, vec!["Alpha", "beta", "gamma"]);
}

#[test]
fn get_missing_host_is_none() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.get_host(uuid::Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn delete_host_cascades_children_and_clears_jump_references() {
    let store = Store::open_in_memory().unwrap();
    let jump = full_host();
    let mut target = Host::new("target", "10.0.0.9");
    target.jump_host_id = Some(jump.id);
    store.save_host(&jump).unwrap();
    store.save_host(&target).unwrap();

    store.delete_host(jump.id).unwrap();

    assert!(store.get_host(jump.id).unwrap().is_none());
    assert_eq!(store.get_host(target.id).unwrap().unwrap().jump_host_id, None);
    let tags: i64 = store.conn().query_row("SELECT COUNT(*) FROM host_tags", [], |r| r.get(0)).unwrap();
    let fwds: i64 = store.conn().query_row("SELECT COUNT(*) FROM port_forwards", [], |r| r.get(0)).unwrap();
    assert_eq!((tags, fwds), (0, 0));
}

#[test]
fn profile_roundtrip_usage_and_delete_in_use() {
    let store = Store::open_in_memory().unwrap();
    let profile = LoginProfile::new(
        "admin",
        "root",
        AuthConfig { method: AuthMethod::KeyboardInteractive, key_path: None, has_secret: false },
    );
    store.save_profile(&profile).unwrap();
    let loaded = store.get_profile(profile.id).unwrap().unwrap();
    assert_eq!(loaded.auth, profile.auth);
    assert_eq!(loaded.name, "admin");
    assert_eq!(store.list_profiles().unwrap().len(), 1);

    let mut host = Host::new("uses-profile", "h");
    host.auth = HostAuth::Profile { profile_id: profile.id };
    store.save_host(&host).unwrap();
    assert_eq!(store.get_host(host.id).unwrap().unwrap().auth, host.auth);
    assert_eq!(store.profile_usage(profile.id).unwrap(), 1);

    let err = store.delete_profile(profile.id).unwrap_err();
    assert_eq!(err, CoreError::store("profile in use"));

    store.delete_host(host.id).unwrap();
    assert_eq!(store.profile_usage(profile.id).unwrap(), 0);
    store.delete_profile(profile.id).unwrap();
    assert!(store.get_profile(profile.id).unwrap().is_none());
}

#[test]
fn groups_sorted_and_delete_group_nulls_host_group() {
    let store = Store::open_in_memory().unwrap();
    let b = Group::new("B", 2);
    let a = Group::new("A", 1);
    store.save_group(&b).unwrap();
    store.save_group(&a).unwrap();
    assert_eq!(store.list_groups().unwrap(), vec![a.clone(), b.clone()]);

    let mut host = Host::new("in-group", "h");
    host.group_id = Some(a.id);
    store.save_host(&host).unwrap();
    store.delete_group(a.id).unwrap();

    assert_eq!(store.list_groups().unwrap(), vec![b]);
    assert_eq!(store.get_host(host.id).unwrap().unwrap().group_id, None);
}

#[test]
fn settings_get_and_set() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_setting(THEME).unwrap(), None);
    store.set_setting(THEME, "dark").unwrap();
    store.set_setting(THEME, "light").unwrap();
    assert_eq!(store.get_setting(THEME).unwrap(), Some("light".to_string()));
}

#[test]
fn snippet_roundtrip_keeps_created_at_and_sorts_by_name() {
    let store = Store::open_in_memory().unwrap();
    let mut beta = Snippet::new("beta", "echo b\necho c", None);
    beta.created_at = 1_000;
    beta.updated_at = 1_000;
    let alpha = Snippet::new("Alpha", "ls", None);
    store.save_snippet(&beta).unwrap();
    store.save_snippet(&alpha).unwrap();
    beta.command = "echo changed".into();
    beta.created_at = 5_000;
    store.save_snippet(&beta).unwrap();

    let names: Vec<String> = store.list_snippets().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Alpha", "beta"]);
    let loaded = store.get_snippet(beta.id).unwrap().unwrap();
    assert_eq!(loaded.command, "echo changed");
    assert_eq!(loaded.created_at, 1_000);
    assert!(loaded.updated_at >= now_unix() - 5);
}

#[test]
fn snippet_for_unknown_host_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let snippet = Snippet::new("x", "ls", Some(Id::new_v4()));
    assert!(matches!(store.save_snippet(&snippet), Err(CoreError::Config { .. })));
    assert!(store.list_snippets().unwrap().is_empty());
}

#[test]
fn deleting_a_host_deletes_only_its_snippets() {
    let store = Store::open_in_memory().unwrap();
    let host = Host::new("web", "10.0.0.5");
    store.save_host(&host).unwrap();
    let bound = Snippet::new("bound", "ls", Some(host.id));
    let global = Snippet::new("global", "ls", None);
    store.save_snippet(&bound).unwrap();
    store.save_snippet(&global).unwrap();

    store.delete_host(host.id).unwrap();
    assert_eq!(store.list_snippets().unwrap(), vec![store.get_snippet(global.id).unwrap().unwrap()]);

    store.delete_snippet(global.id).unwrap();
    store.delete_snippet(global.id).unwrap();
    assert!(store.list_snippets().unwrap().is_empty());
}

#[test]
fn bookmarks_are_kept_per_host_and_local() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    store.add_bookmark(None, "/home/u").unwrap();
    store.add_bookmark(Some(a), "/srv").unwrap();
    store.add_bookmark(Some(b), "/etc").unwrap();
    let paths = |h| store.list_bookmarks(h).unwrap().into_iter().map(|b| b.path).collect::<Vec<_>>();
    assert_eq!(paths(None), ["/home/u"]);
    assert_eq!(paths(Some(a)), ["/srv"]);
    assert_eq!(paths(Some(b)), ["/etc"]);
}

#[test]
fn adding_a_bookmark_twice_returns_the_existing_one() {
    let store = Store::open_in_memory().unwrap();
    let first = store.add_bookmark(None, "/srv").unwrap();
    let again = store.add_bookmark(None, "/srv").unwrap();
    assert_eq!(first, again);
    assert_eq!(store.list_bookmarks(None).unwrap().len(), 1);
}

#[test]
fn bookmarks_keep_insertion_order() {
    let store = Store::open_in_memory().unwrap();
    for p in ["/z", "/a", "/m"] {
        store.add_bookmark(None, p).unwrap();
    }
    let paths: Vec<_> = store.list_bookmarks(None).unwrap().into_iter().map(|b| b.path).collect();
    assert_eq!(paths, ["/z", "/a", "/m"]);
}

#[test]
fn removing_bookmarks_including_unknown_ones() {
    let store = Store::open_in_memory().unwrap();
    let b = store.add_bookmark(None, "/srv").unwrap();
    store.remove_bookmark(uuid::Uuid::new_v4()).unwrap();
    store.remove_bookmark(b.id).unwrap();
    assert!(store.list_bookmarks(None).unwrap().is_empty());
}

#[test]
fn deleting_a_host_deletes_its_bookmarks() {
    let store = Store::open_in_memory().unwrap();
    let host = Host::new("web", "10.0.0.1");
    store.save_host(&host).unwrap();
    store.add_bookmark(Some(host.id), "/srv").unwrap();
    store.add_bookmark(None, "/home").unwrap();
    store.delete_host(host.id).unwrap();
    assert!(store.list_bookmarks(Some(host.id)).unwrap().is_empty());
    assert_eq!(store.list_bookmarks(None).unwrap().len(), 1);
}

#[test]
fn bookmark_paths_are_validated() {
    let store = Store::open_in_memory().unwrap();
    for bad in ["rel/x".to_string(), "/a\nb".to_string(), format!("/{}", "a".repeat(4096))] {
        assert!(store.add_bookmark(None, &bad).is_err(), "{bad:?}");
    }
    assert!(store.list_bookmarks(None).unwrap().is_empty());
}
