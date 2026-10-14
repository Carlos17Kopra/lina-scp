//! Versioned schema migrations. `PRAGMA user_version` holds the applied version.
//! Never edit a released migration; append a new one instead.

use rusqlite::Connection;

use crate::error::{CoreError, Result};

const V1: &str = r#"
CREATE TABLE groups (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE login_profiles (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    user        TEXT NOT NULL,
    method      TEXT NOT NULL CHECK (method IN ('password', 'key', 'agent', 'keyboard_interactive')),
    key_path    TEXT,
    has_secret  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE hosts (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    address           TEXT NOT NULL,
    port              INTEGER NOT NULL DEFAULT 22,
    user              TEXT,
    group_id          TEXT,
    color             TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    protocol_ssh      INTEGER NOT NULL DEFAULT 1,
    protocol_sftp     INTEGER NOT NULL DEFAULT 1,
    login_profile_id  TEXT,
    auth_method       TEXT CHECK (auth_method IN ('password', 'key', 'agent', 'keyboard_interactive')),
    key_path          TEXT,
    has_secret        INTEGER NOT NULL DEFAULT 0,
    jump_host_id      TEXT,
    keepalive_s       INTEGER NOT NULL DEFAULT 30,
    timeout_s         INTEGER NOT NULL DEFAULT 15,
    start_command     TEXT,
    env               TEXT NOT NULL DEFAULT '[]',
    sftp_settings     TEXT NOT NULL DEFAULT '{}',
    source            TEXT NOT NULL DEFAULT 'linascp' CHECK (source IN ('linascp', 'ssh_config')),
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    CHECK ((login_profile_id IS NULL) <> (auth_method IS NULL))
);
CREATE INDEX hosts_group_id ON hosts(group_id);
CREATE INDEX hosts_login_profile_id ON hosts(login_profile_id);

CREATE TABLE host_tags (
    host_id  TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (host_id, tag)
);

CREATE TABLE port_forwards (
    id         TEXT PRIMARY KEY NOT NULL,
    host_id    TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    type       TEXT NOT NULL CHECK (type IN ('L', 'R', 'D')),
    bind       TEXT NOT NULL,
    target     TEXT NOT NULL,
    autostart  INTEGER NOT NULL DEFAULT 0,
    position   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX port_forwards_host_id ON port_forwards(host_id);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
);
"#;

const MIGRATIONS: &[&str] = &[V1];

/// Latest schema version known to this build.
pub(crate) const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

/// Applies all pending migrations, each in its own transaction.
pub(crate) fn run(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > LATEST_VERSION {
        return Err(CoreError::store(format!(
            "database schema version {current} is newer than supported version {LATEST_VERSION}"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    Ok(())
}
