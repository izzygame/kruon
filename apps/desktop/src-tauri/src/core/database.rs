use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction};

use super::error::{KruonError, KruonResult};

pub const CURRENT_SCHEMA_VERSION: i64 = 4;

pub fn open_local_database(path: impl AsRef<Path>) -> KruonResult<Connection> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| KruonError::Store("database path must have a parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    reject_symlink(parent, "database directory")?;
    if path.exists() {
        reject_symlink(path, "database file")?;
    }
    harden_directory_permissions(parent)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    harden_file_permissions(path)?;
    Ok(connection)
}

fn reject_symlink(path: &Path, label: &str) -> KruonResult<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(KruonError::Store(format!(
            "{label} must not be a symbolic link"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn harden_directory_permissions(path: &Path) -> KruonResult<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_directory_permissions(_path: &Path) -> KruonResult<()> {
    Ok(())
}

#[cfg(unix)]
fn harden_file_permissions(path: &Path) -> KruonResult<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_file_permissions(_path: &Path) -> KruonResult<()> {
    Ok(())
}

pub fn ensure_supported_schema(connection: &Connection) -> KruonResult<()> {
    let has_migration_table: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = 'schema_migrations'
        )",
        [],
        |row| row.get(0),
    )?;
    if !has_migration_table {
        return Ok(());
    }
    let newest = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()?
        .flatten();
    if newest.is_some_and(|version| version > CURRENT_SCHEMA_VERSION) {
        return Err(KruonError::Store(
            "database schema is newer than this application".into(),
        ));
    }
    Ok(())
}

pub fn run_migration<F>(connection: &mut Connection, migration: F) -> KruonResult<()>
where
    F: FnOnce(&Transaction<'_>) -> KruonResult<()>,
{
    let transaction = connection.transaction()?;
    migration(&transaction)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_database_opens_with_nofollow_guard() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("private.sqlite3");
        let connection = open_local_database(&database).unwrap();
        connection
            .execute("CREATE TABLE proof(value TEXT)", [])
            .unwrap();
        assert!(database.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn local_database_is_private_and_rejects_symlink_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("private.sqlite3");
        drop(open_local_database(&database).unwrap());
        assert_eq!(
            std::fs::metadata(directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&database).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let target = directory.path().join("target.sqlite3");
        std::fs::write(&target, "not a database").unwrap();
        let link = directory.path().join("linked.sqlite3");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            open_local_database(&link),
            Err(KruonError::Store(_))
        ));
    }

    #[test]
    fn injected_upgrade_failure_rolls_back_schema_and_data() {
        let mut connection = Connection::open_in_memory().unwrap();
        let failed = run_migration(&mut connection, |transaction| {
            transaction.execute_batch(
                "CREATE TABLE alpha_upgrade(value TEXT NOT NULL);
                 INSERT INTO alpha_upgrade(value) VALUES ('partial');",
            )?;
            Err(KruonError::Store("injected migration failure".into()))
        });
        assert!(matches!(failed, Err(KruonError::Store(_))));

        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'alpha_upgrade'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
    }

    #[test]
    fn future_schema_is_rejected_without_mutating_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("future.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations(version, applied_at) VALUES (5, 'future');
                CREATE TABLE future_sentinel(value TEXT NOT NULL);
                INSERT INTO future_sentinel(value) VALUES ('untouched');",
            )
            .unwrap();
        drop(connection);

        let connection = Connection::open(&database).unwrap();
        assert!(matches!(
            ensure_supported_schema(&connection),
            Err(KruonError::Store(_))
        ));
        assert_eq!(
            connection
                .query_row("SELECT value FROM future_sentinel", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "untouched"
        );
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            5
        );
    }
}
