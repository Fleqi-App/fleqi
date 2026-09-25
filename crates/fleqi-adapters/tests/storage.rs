//! M1.2 存储测试：真实 SQLite 文件——保存/重启恢复、版本冲突、损坏不覆盖、迁移前备份、迁移失败保留原数据。

use fleqi_adapters::storage::{
    BACKUP_DIR, DATABASE_FILE, Database, MIGRATIONS, Migration, SqliteSettingsStore,
};
use fleqi_application::ports::{SettingsStore, StorageError};
use fleqi_domain::idempotency::Receipt;
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{Settings, Theme};
use std::sync::Arc;

fn receipt(id: &str) -> Receipt {
    Receipt {
        request_id: id.into(),
        fingerprint: format!("fp-{id}"),
        result_json: "{}".into(),
    }
}

fn shutdown(db: Arc<Database>) {
    match Arc::try_unwrap(db) {
        Ok(db) => db.shutdown(),
        Err(_) => panic!("仍有其他持有者"),
    }
}

#[test]
fn fresh_database_migrates_saves_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = Arc::new(Database::open(dir.path()).unwrap());
        assert_eq!(db.schema_version(), 5);
        assert_eq!(
            db.applied_migrations(),
            [
                "1:m1_settings_and_receipts",
                "2:m2_sessions_and_entries",
                "3:m3_runs_rules_favorites_history",
                "4:m3_installed_tools",
                "5:m3_providers",
            ]
        );
        assert!(db.last_backup().is_none(), "全新数据库无需备份");
        let store = SqliteSettingsStore::new(db.clone());
        assert!(store.load().unwrap().is_none());

        let settings = Settings {
            theme: Theme::Light,
            ..Settings::default()
        };
        let rev = store
            .commit(&settings, Some(Revision::new(0)), &receipt("r1"))
            .unwrap();
        assert_eq!(rev, Revision::new(1));
        assert_eq!(
            store.find_receipt("r1").unwrap().unwrap().fingerprint,
            "fp-r1"
        );
        assert!(store.find_receipt("missing").unwrap().is_none());
        drop(store);
        shutdown(db);
    }
    // 重启恢复：真实文件重新打开。
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let store = SqliteSettingsStore::new(db);
    let persisted = store.load().unwrap().unwrap();
    assert_eq!(persisted.revision, Revision::new(1));
    assert_eq!(persisted.settings.theme, Theme::Light);
}

#[test]
fn commit_with_stale_expected_revision_is_rejected_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let store = SqliteSettingsStore::new(db);
    store
        .commit(&Settings::default(), Some(Revision::new(0)), &receipt("a"))
        .unwrap();
    let error = store
        .commit(&Settings::default(), Some(Revision::new(0)), &receipt("b"))
        .unwrap_err();
    assert_eq!(
        error,
        StorageError::RevisionMismatch {
            current: Revision::new(1)
        }
    );
    assert!(
        store.find_receipt("b").unwrap().is_none(),
        "回执与设置同事务，失败不落回执"
    );
}

#[test]
fn corrupt_database_is_reported_and_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    let garbage: Vec<u8> = (0..4096u32).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(&path, &garbage).unwrap();
    let error = Database::open(dir.path()).unwrap_err();
    assert!(
        matches!(
            error,
            StorageError::Corrupt(_) | StorageError::Unavailable(_)
        ),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), garbage, "损坏文件保持原样");
}

const V2: Migration = Migration {
    version: 6,
    name: "test_add_table",
    sql: "CREATE TABLE test_extra (id INTEGER PRIMARY KEY);",
};
const V2_BROKEN: Migration = Migration {
    version: 7,
    name: "test_broken",
    sql: "CREATE TABLE ok_table (id INTEGER); THIS IS NOT SQL;",
};

#[test]
fn pending_migration_backs_up_first_and_failure_keeps_original_data() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = Arc::new(Database::open(dir.path()).unwrap());
        SqliteSettingsStore::new(db.clone())
            .commit(
                &Settings::default(),
                Some(Revision::new(0)),
                &receipt("seed"),
            )
            .unwrap();
        shutdown(db);
    }

    let mut broken = MIGRATIONS.to_vec();
    broken.push(V2_BROKEN); // version 5 broken
    let error = Database::open_with_migrations(dir.path(), &broken).unwrap_err();
    assert!(
        matches!(
            error,
            StorageError::Unavailable(_) | StorageError::Corrupt(_)
        ),
        "{error:?}"
    );
    let backups: Vec<_> = std::fs::read_dir(dir.path().join(BACKUP_DIR))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1, "迁移前已生成一致副本");

    let db = Arc::new(Database::open(dir.path()).unwrap());
    assert_eq!(db.schema_version(), 5, "失败迁移已回滚");
    assert_eq!(
        SqliteSettingsStore::new(db.clone())
            .load()
            .unwrap()
            .unwrap()
            .revision,
        Revision::new(1),
        "原数据保留"
    );
    shutdown(db);

    let mut good = MIGRATIONS.to_vec();
    good.push(V2);
    let db = Database::open_with_migrations(dir.path(), &good).unwrap();
    assert_eq!(db.schema_version(), 6);
    assert!(
        db.last_backup()
            .unwrap()
            .starts_with(dir.path().join(BACKUP_DIR))
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join(BACKUP_DIR))
            .unwrap()
            .count(),
        2
    );
}
