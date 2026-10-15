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

/// TP2a: per-host terminal scheme / font size overrides (NULL = global setting).
const V2: &str = r#"
ALTER TABLE hosts ADD COLUMN terminal_scheme TEXT;
ALTER TABLE hosts ADD COLUMN terminal_font_size INTEGER;
"#;

/// TP2b: snippets, global (host_id NULL) or bound to one host.
const V3: &str = r#"
CREATE TABLE snippets (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    host_id     TEXT REFERENCES hosts(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE INDEX snippets_host_id ON snippets(host_id);
"#;

const MIGRATIONS: &[&str] = &[V1, V2, V3];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_adds_nullable_terminal_columns_to_existing_hosts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.execute_batch("PRAGMA user_version = 1").unwrap();
        conn.execute(
            "INSERT INTO hosts (id, name, address, auth_method, created_at, updated_at) \
             VALUES ('a', 'n', 'h', 'password', 0, 0)",
            [],
        )
        .unwrap();
        run(&mut conn).unwrap();
        let row: (Option<String>, Option<i64>) = conn
            .query_row("SELECT terminal_scheme, terminal_font_size FROM hosts WHERE id = 'a'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (None, None));
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, LATEST_VERSION);
    }
}
