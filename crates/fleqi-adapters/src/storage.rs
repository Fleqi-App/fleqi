//! SQLite 存储（architecture.md §9.3、§12.3）：rusqlite bundled，专用线程排序全部访问，
//! 开启外键、WAL 与事务；迁移前用 backup API 保存一致副本；损坏/迁移失败保留原数据。

use fleqi_application::ports::{
    Clock, PersistedSettings, SessionStore, SettingsStore, StorageError, SystemClock,
};
use fleqi_domain::idempotency::Receipt;
use fleqi_domain::revision::Revision;
use fleqi_domain::session::{ConversationEntry, Session};
use fleqi_domain::settings::Settings;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

pub const DATABASE_FILE: &str = "fleqi.sqlite3";
pub const BACKUP_DIR: &str = "backups";

/// 一次迁移：版本号单调递增；`sql` 在同一事务内执行。
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 首个 schema：settings、schema_migrations、request_receipts（会话等表随业务加入）。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "m1_settings_and_receipts",
        sql: r#"
        CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            revision INTEGER NOT NULL,
            json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS request_receipts (
            request_id TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            result_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
    "#,
    },
    Migration {
        version: 2,
        name: "m2_sessions_and_entries",
        sql: r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            json TEXT NOT NULL,
            state TEXT NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0,
            last_used_at TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_last_used ON sessions(last_used_at DESC);
        CREATE TABLE IF NOT EXISTS conversation_entries (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            run_id TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_entries_session ON conversation_entries(session_id, created_at);
    "#,
    },
    Migration {
        version: 3,
        name: "m3_runs_rules_favorites_history",
        sql: r#"
        CREATE TABLE IF NOT EXISTS runs (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            json TEXT NOT NULL,
            state TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runs_session ON runs(session_id, created_at DESC);
        CREATE TABLE IF NOT EXISTS run_output_segments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
            bytes BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS rules (
            id TEXT PRIMARY KEY,
            json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS favorites (
            id TEXT PRIMARY KEY,
            json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS input_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entry TEXT NOT NULL
        );
    "#,
    },
    Migration {
        version: 4,
        name: "m3_installed_tools",
        sql: r#"
        CREATE TABLE IF NOT EXISTS installed_tools (
            id TEXT PRIMARY KEY,
            manifest_json TEXT NOT NULL,
            installed_at TEXT NOT NULL,
            install_dir TEXT NOT NULL
        );
    "#,
    },
    Migration {
        version: 5,
        name: "m3_providers",
        sql: r#"
        CREATE TABLE IF NOT EXISTS providers (
            id TEXT PRIMARY KEY,
            json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
    "#,
    },
];

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// 单写入服务：所有 SQL 在专用线程按到达顺序执行；长 HTTP/进程等待不持有它。
pub struct Database {
    sender: Sender<Job>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
    schema_version: u32,
    applied: Vec<String>,
    last_backup: Option<PathBuf>,
}

impl Database {
    pub fn open(data_dir: &Path) -> Result<Self, StorageError> {
        Self::open_with_migrations(data_dir, MIGRATIONS)
    }

    /// 测试可注入额外迁移以验证备份与失败回滚。
    pub fn open_with_migrations(
        data_dir: &Path,
        migrations: &[Migration],
    ) -> Result<Self, StorageError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| StorageError::Io(format!("创建数据目录失败：{e}")))?;
        let path = data_dir.join(DATABASE_FILE);
        let existed = path.exists();
        let mut connection = Connection::open(&path)
            .map_err(|e| StorageError::Unavailable(format!("打开数据库失败：{e}")))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| StorageError::Unavailable(e.to_string()))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
            )
            .map_err(|e| classify(e, "初始化 PRAGMA"))?;
        if existed {
            let check: String = connection
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .map_err(|e| classify(e, "quick_check"))?;
            if check != "ok" {
                return Err(StorageError::Corrupt(format!("quick_check：{check}")));
            }
        }
        let (schema_version, applied, last_backup) =
            migrate(&mut connection, &path, existed, migrations)?;

        let (sender, receiver) = channel::<Job>();
        let worker = std::thread::Builder::new()
            .name("fleqi-sqlite-writer".into())
            .spawn(move || {
                let mut connection = connection;
                for job in receiver {
                    job(&mut connection);
                }
                let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            })
            .map_err(|e| StorageError::Unavailable(format!("启动写线程失败：{e}")))?;
        Ok(Self {
            sender,
            worker: Some(worker),
            path,
            schema_version,
            applied,
            last_backup,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn applied_migrations(&self) -> &[String] {
        &self.applied
    }

    pub fn last_backup(&self) -> Option<&Path> {
        self.last_backup.as_deref()
    }

    /// 在写线程上执行并等待结果。
    pub fn run<R, F>(&self, job: F) -> Result<R, StorageError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, StorageError> + Send + 'static,
    {
        let (reply, receive) = channel();
        self.sender
            .send(Box::new(move |connection| {
                let _ = reply.send(job(connection));
            }))
            .map_err(|_| StorageError::Unavailable("写线程已停止".into()))?;
        receive
            .recv()
            .map_err(|_| StorageError::Unavailable("写线程未返回结果".into()))?
    }

    /// 排空队列：等待此前全部写入完成（退出前调用；共享持有时无法 shutdown，用此保证落盘）。
    pub fn drain(&self) -> Result<(), StorageError> {
        self.run(|connection| {
            connection
                .execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
                .map_err(|e| classify(e, "checkpoint"))?;
            Ok(())
        })
    }

    /// 排空队列并停止写线程（退出时调用；之后 run 返回 Unavailable）。
    pub fn shutdown(mut self) {
        drop(std::mem::replace(&mut self.sender, channel().0));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .field("schema_version", &self.schema_version)
            .finish()
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            drop(std::mem::replace(&mut self.sender, channel().0));
            let _ = worker.join();
        }
    }
}

fn classify(error: rusqlite::Error, context: &str) -> StorageError {
    let text = error.to_string();
    if text.contains("malformed") || text.contains("not a database") || text.contains("corrupt") {
        StorageError::Corrupt(format!("{context}：{text}"))
    } else if text.contains("disk I/O")
        || text.contains("unable to open")
        || text.contains("readonly")
    {
        StorageError::Io(format!("{context}：{text}"))
    } else {
        StorageError::Unavailable(format!("{context}：{text}"))
    }
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// 迁移：先读已应用版本；若有待应用迁移且数据库已存在，用 backup API 复制一致副本；
/// 每个迁移在事务内执行并写 schema_migrations；失败回滚，原数据保留。
fn migrate(
    connection: &mut Connection,
    path: &Path,
    existed: bool,
    migrations: &[Migration],
) -> Result<(u32, Vec<String>, Option<PathBuf>), StorageError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL);",
        )
        .map_err(|e| classify(e, "schema_migrations"))?;
    let mut applied: Vec<(u32, String)> = {
        let mut stmt = connection
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .map_err(|e| classify(e, "读取迁移记录"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| classify(e, "读取迁移记录"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| classify(e, "读取迁移记录"))?
    };
    let current = applied.last().map(|(v, _)| *v).unwrap_or(0);
    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    let mut last_backup = None;
    if !pending.is_empty() && existed && current > 0 {
        last_backup = Some(backup(connection, path, current)?);
    }
    for migration in pending {
        let tx = connection
            .transaction()
            .map_err(|e| classify(e, "开启迁移事务"))?;
        tx.execute_batch(migration.sql)
            .map_err(|e| classify(e, &format!("迁移 {} 失败", migration.name)))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, now()],
        )
        .map_err(|e| classify(e, "记录迁移"))?;
        tx.commit().map_err(|e| classify(e, "提交迁移"))?;
        applied.push((migration.version, migration.name.to_owned()));
    }
    let version = applied.last().map(|(v, _)| *v).unwrap_or(0);
    Ok((
        version,
        applied
            .into_iter()
            .map(|(v, n)| format!("{v}:{n}"))
            .collect(),
        last_backup,
    ))
}

/// 用 SQLite backup API 生成一致副本，不能直接复制活动 WAL 数据库。
fn backup(
    connection: &Connection,
    path: &Path,
    from_version: u32,
) -> Result<PathBuf, StorageError> {
    let dir = path.parent().unwrap_or(Path::new(".")).join(BACKUP_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| StorageError::Io(format!("创建备份目录失败：{e}")))?;
    let stamp = now().replace([':', '-'], "");
    let target = dir.join(format!("fleqi-v{from_version}-{stamp}.sqlite3"));
    let mut destination =
        Connection::open(&target).map_err(|e| StorageError::Io(format!("创建备份失败：{e}")))?;
    let backup = rusqlite::backup::Backup::new(connection, &mut destination)
        .map_err(|e| StorageError::Io(format!("备份初始化失败：{e}")))?;
    backup
        .run_to_completion(64, std::time::Duration::from_millis(5), None)
        .map_err(|e| StorageError::Io(format!("备份写入失败：{e}")))?;
    Ok(target)
}

/// 设置与回执在同一事务提交。
pub struct SqliteSettingsStore {
    database: Arc<Database>,
}

impl SqliteSettingsStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

fn current_revision(connection: &Connection) -> Result<Revision, StorageError> {
    let revision: Option<i64> = connection
        .query_row("SELECT revision FROM settings WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| classify(e, "读取设置版本"))?;
    Ok(Revision::new(revision.unwrap_or(0) as u64))
}

impl SettingsStore for SqliteSettingsStore {
    fn load(&self) -> Result<Option<PersistedSettings>, StorageError> {
        self.database.run(|connection| {
            let row: Option<(i64, String)> = connection
                .query_row(
                    "SELECT revision, json FROM settings WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| classify(e, "读取设置"))?;
            match row {
                None => Ok(None),
                Some((revision, json)) => {
                    let settings: Settings = serde_json::from_str(&json)
                        .map_err(|e| StorageError::Corrupt(format!("设置记录无法解析：{e}")))?;
                    Ok(Some(PersistedSettings {
                        settings,
                        revision: Revision::new(revision as u64),
                    }))
                }
            }
        })
    }

    fn commit(
        &self,
        settings: &Settings,
        expected: Option<Revision>,
        receipt: &Receipt,
    ) -> Result<Revision, StorageError> {
        let json = serde_json::to_string(settings)
            .map_err(|e| StorageError::Io(format!("序列化设置失败：{e}")))?;
        let receipt = receipt.clone();
        self.database.run(move |connection| {
            let tx = connection.transaction().map_err(|e| classify(e, "开启事务"))?;
            let current = current_revision(&tx)?;
            if let Some(expected) = expected
                && expected != current
            {
                return Err(StorageError::RevisionMismatch { current });
            }
            let next = current.next();
            tx.execute(
                "INSERT INTO settings (id, revision, json, updated_at) VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET revision = excluded.revision, json = excluded.json, updated_at = excluded.updated_at",
                params![next.value() as i64, json, now()],
            )
            .map_err(|e| classify(e, "写入设置"))?;
            tx.execute(
                "INSERT INTO request_receipts (request_id, fingerprint, result_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![receipt.request_id, receipt.fingerprint, receipt.result_json, now()],
            )
            .map_err(|e| classify(e, "写入回执"))?;
            tx.commit().map_err(|e| classify(e, "提交事务"))?;
            Ok(next)
        })
    }

    fn find_receipt(&self, request_id: &str) -> Result<Option<Receipt>, StorageError> {
        let request_id = request_id.to_owned();
        self.database.run(move |connection| {
            connection
                .query_row(
                    "SELECT request_id, fingerprint, result_json FROM request_receipts WHERE request_id = ?1",
                    params![request_id],
                    |row| Ok(Receipt { request_id: row.get(0)?, fingerprint: row.get(1)?, result_json: row.get(2)? }),
                )
                .optional()
                .map_err(|e| classify(e, "读取回执"))
        })
    }
}

/// 会话与对话记录：JSON 载荷 + 排序/过滤列；删除为事务级联。
pub struct SqliteSessionStore {
    database: Arc<Database>,
}

impl SqliteSessionStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl SessionStore for SqliteSessionStore {
    fn find_receipt(&self, request_id: &str) -> Result<Option<Receipt>, StorageError> {
        let request_id = request_id.to_owned();
        self.database.run(move |connection| {
            connection.query_row(
                "SELECT request_id, fingerprint, result_json FROM request_receipts WHERE request_id = ?1",
                params![request_id],
                |row| Ok(Receipt { request_id: row.get(0)?, fingerprint: row.get(1)?, result_json: row.get(2)? }),
            ).optional().map_err(|e| classify(e, "读取会话请求回执"))
        })
    }

    fn commit_request(
        &self,
        sessions: &[Session],
        deleted_ids: &[String],
        receipt: &Receipt,
    ) -> Result<Receipt, StorageError> {
        let sessions = sessions
            .iter()
            .map(|session| {
                serde_json::to_string(session)
                    .map(|json| (session.clone(), json))
                    .map_err(|e| StorageError::Io(format!("序列化会话失败：{e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let deleted_ids = deleted_ids.to_vec();
        let receipt = receipt.clone();
        self.database.run(move |connection| {
            let tx = connection.transaction().map_err(|e| classify(e, "开启会话请求事务"))?;
            let previous = tx.query_row(
                "SELECT request_id, fingerprint, result_json FROM request_receipts WHERE request_id = ?1",
                params![receipt.request_id],
                |row| Ok(Receipt { request_id: row.get(0)?, fingerprint: row.get(1)?, result_json: row.get(2)? }),
            ).optional().map_err(|e| classify(e, "检查会话请求回执"))?;
            if let Some(previous) = previous {
                return Ok(previous);
            }
            for (session, json) in sessions {
                tx.execute(
                    "INSERT INTO sessions (id, json, state, pinned, last_used_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json, state = excluded.state, pinned = excluded.pinned, last_used_at = excluded.last_used_at",
                    params![session.id, json, format!("{:?}", session.state).to_lowercase(), session.pinned as i32, session.last_used_at, session.created_at],
                ).map_err(|e| classify(e, "提交请求会话"))?;
            }
            for id in deleted_ids {
                tx.execute("DELETE FROM conversation_entries WHERE session_id = ?1", params![id])
                    .map_err(|e| classify(e, "删除请求会话对话"))?;
                tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])
                    .map_err(|e| classify(e, "删除请求会话"))?;
            }
            tx.execute(
                "INSERT INTO request_receipts (request_id, fingerprint, result_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![receipt.request_id, receipt.fingerprint, receipt.result_json, now()],
            ).map_err(|e| classify(e, "写入会话请求回执"))?;
            tx.commit().map_err(|e| classify(e, "提交会话请求事务"))?;
            Ok(receipt)
        })
    }

    fn load_all(&self) -> Result<Vec<Session>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM sessions ORDER BY last_used_at DESC")
                .map_err(|e| classify(e, "读取会话"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取会话"))?;
            let mut sessions = Vec::new();
            for json in rows {
                let json = json.map_err(|e| classify(e, "读取会话"))?;
                match serde_json::from_str::<Session>(&json) {
                    Ok(session) => sessions.push(session),
                    // 损坏单条记录不阻止启动：跳过并继续（FR-DATA-004）。
                    Err(_) => continue,
                }
            }
            Ok(sessions)
        })
    }

    fn upsert(&self, session: &Session) -> Result<(), StorageError> {
        let json = serde_json::to_string(session)
            .map_err(|e| StorageError::Io(format!("序列化会话失败：{e}")))?;
        let session = session.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO sessions (id, json, state, pinned, last_used_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json, state = excluded.state, pinned = excluded.pinned, last_used_at = excluded.last_used_at",
                    params![
                        session.id,
                        json,
                        format!("{:?}", session.state).to_lowercase(),
                        session.pinned as i32,
                        session.last_used_at,
                        session.created_at
                    ],
                )
                .map_err(|e| classify(e, "写入会话"))?;
            Ok(())
        })
    }

    fn delete(&self, session_id: &str) -> Result<(), StorageError> {
        let session_id = session_id.to_owned();
        self.database.run(move |connection| {
            let tx = connection
                .transaction()
                .map_err(|e| classify(e, "开启事务"))?;
            tx.execute(
                "DELETE FROM conversation_entries WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| classify(e, "删除对话"))?;
            tx.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
                .map_err(|e| classify(e, "删除会话"))?;
            tx.commit().map_err(|e| classify(e, "提交删除"))?;
            Ok(())
        })
    }

    fn append_entry(&self, entry: &ConversationEntry) -> Result<(), StorageError> {
        let entry = entry.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO conversation_entries (id, session_id, role, content, run_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![entry.id, entry.session_id, format!("{:?}", entry.role).to_lowercase(), entry.content, entry.run_id, entry.created_at],
                )
                .map_err(|e| classify(e, "写入对话"))?;
            Ok(())
        })
    }

    fn entries(
        &self,
        session_id: &str,
        limit: usize,
        before: Option<&str>,
    ) -> Result<Vec<ConversationEntry>, StorageError> {
        let session_id = session_id.to_owned();
        let before = before.map(|s| s.to_owned());
        self.database.run(move |connection| {
            let mut stmt = connection
                .prepare(
                    "SELECT id, session_id, role, content, run_id, created_at FROM conversation_entries
                     WHERE session_id = ?1 AND (?2 IS NULL OR created_at < ?2) ORDER BY created_at DESC LIMIT ?3",
                )
                .map_err(|e| classify(e, "读取对话"))?;
            let rows = stmt
                .query_map(params![session_id, before, limit as i64], |row| {
                    let role: String = row.get(2)?;
                    Ok(ConversationEntry {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        role: match role.as_str() {
                            "user" => fleqi_domain::session::EntryRole::User,
                            "manualcommand" => fleqi_domain::session::EntryRole::ManualCommand,
                            "assistant" => fleqi_domain::session::EntryRole::Assistant,
                            _ => fleqi_domain::session::EntryRole::System,
                        },
                        content: row.get(3)?,
                        run_id: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })
                .map_err(|e| classify(e, "读取对话"))?;
            let mut entries = rows.collect::<Result<Vec<_>, _>>().map_err(|e| classify(e, "读取对话"))?;
            entries.reverse();
            Ok(entries)
        })
    }
}

/// Run 记录 + 输出分段存储。
pub struct SqliteRunStore {
    database: Arc<Database>,
}

impl SqliteRunStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl fleqi_application::ports::RunStore for SqliteRunStore {
    fn find_request(
        &self,
        request_id: &str,
    ) -> Result<Option<fleqi_application::run_service::RunRecord>, StorageError> {
        let request_id = request_id.to_owned();
        self.database.run(move |connection| {
            let json: Option<String> = connection
                .query_row(
                    "SELECT json FROM runs WHERE json_extract(json, '$.requestId') = ?1 LIMIT 1",
                    params![request_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| classify(e, "读取任务请求回执"))?;
            json.map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| StorageError::Corrupt(format!("Run 记录损坏：{e}")))
            })
            .transpose()
        })
    }
    fn interrupt_unfinished(&self, now: &str) -> Result<(), StorageError> {
        let now = now.to_owned();
        self.database.run(move |connection| {
            connection.execute(
                "UPDATE runs SET state = 'interrupted', json = json_set(json, '$.state', 'interrupted', '$.updatedAt', ?1) WHERE json_extract(json, '$.state') IN ('queued', 'running', 'awaitingApproval', 'planning', 'awaitingInput', 'installing')",
                params![now]
            ).map_err(|e| classify(e, "中断遗留任务"))?;
            Ok(())
        })
    }
    fn load_all(&self) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM runs ORDER BY created_at")
                .map_err(|e| classify(e, "读取全部任务"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取全部任务"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| classify(e, "读取全部任务"))?;
            rows.into_iter()
                .map(|json| {
                    serde_json::from_str(&json).map_err(|e| StorageError::Corrupt(e.to_string()))
                })
                .collect()
        })
    }

    fn upsert(&self, run: &fleqi_application::run_service::RunRecord) -> Result<(), StorageError> {
        let json = serde_json::to_string(run)
            .map_err(|e| StorageError::Io(format!("序列化 Run 失败：{e}")))?;
        let run = run.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO runs (id, session_id, json, state, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json, state = excluded.state",
                    params![run.id, run.session_id, json, format!("{:?}", run.state).to_lowercase(), run.created_at],
                )
                .map_err(|e| classify(e, "写入 Run"))?;
            Ok(())
        })
    }

    fn get(
        &self,
        run_id: &str,
    ) -> Result<Option<fleqi_application::run_service::RunRecord>, StorageError> {
        let run_id = run_id.to_owned();
        self.database.run(move |connection| {
            let json: Option<String> = connection
                .query_row(
                    "SELECT json FROM runs WHERE id = ?1",
                    params![run_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| classify(e, "读取 Run"))?;
            json.map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| StorageError::Corrupt(format!("Run 记录损坏：{e}")))
            })
            .transpose()
        })
    }

    fn list(
        &self,
        session_id: &str,
    ) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        let session_id = session_id.to_owned();
        self.database.run(move |connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM runs WHERE session_id = ?1 ORDER BY created_at DESC")
                .map_err(|e| classify(e, "读取 Run 列表"))?;
            let rows = stmt
                .query_map(params![session_id], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取 Run 列表"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| classify(e, "读取 Run 列表"))?
                .into_iter()
                .map(|json| {
                    serde_json::from_str(&json)
                        .map_err(|e| StorageError::Corrupt(format!("Run 记录损坏：{e}")))
                })
                .collect()
        })
    }

    fn append_output(&self, run_id: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let run_id = run_id.to_owned();
        let bytes = bytes.to_vec();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO run_output_segments (run_id, bytes) VALUES (?1, ?2)",
                    params![run_id, bytes],
                )
                .map_err(|e| classify(e, "追加 Run 输出"))?;
            Ok(())
        })
    }
}

/// 规则/收藏/输入历史存储。
pub struct SqliteCollectionStore {
    database: Arc<Database>,
}

impl SqliteCollectionStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl fleqi_application::ports::CollectionStore for SqliteCollectionStore {
    fn load_rules(&self) -> Result<Vec<fleqi_application::collection_service::Rule>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM rules ORDER BY updated_at")
                .map_err(|e| classify(e, "读取规则"))?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取规则"))?
                .collect::<Result<_, _>>()
                .map_err(|e| classify(e, "读取规则"))?;
            load_json_rows(rows)
        })
    }

    fn upsert_rule(
        &self,
        rule: &fleqi_application::collection_service::Rule,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(rule)
            .map_err(|e| StorageError::Io(format!("序列化规则失败：{e}")))?;
        let rule = rule.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO rules (id, json, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
                    params![rule.id, json, rule.updated_at],
                )
                .map_err(|e| classify(e, "写入规则"))?;
            Ok(())
        })
    }

    fn delete_rule(&self, rule_id: &str) -> Result<(), StorageError> {
        let rule_id = rule_id.to_owned();
        self.database.run(move |connection| {
            connection
                .execute("DELETE FROM rules WHERE id = ?1", params![rule_id])
                .map_err(|e| classify(e, "删除规则"))?;
            Ok(())
        })
    }

    fn load_favorites(
        &self,
    ) -> Result<Vec<fleqi_application::collection_service::Favorite>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM favorites ORDER BY created_at")
                .map_err(|e| classify(e, "读取收藏"))?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取收藏"))?
                .collect::<Result<_, _>>()
                .map_err(|e| classify(e, "读取收藏"))?;
            load_json_rows(rows)
        })
    }

    fn upsert_favorite(
        &self,
        favorite: &fleqi_application::collection_service::Favorite,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(favorite)
            .map_err(|e| StorageError::Io(format!("序列化收藏失败：{e}")))?;
        let favorite = favorite.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO favorites (id, json, created_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                    params![favorite.id, json, favorite.created_at],
                )
                .map_err(|e| classify(e, "写入收藏"))?;
            Ok(())
        })
    }

    fn delete_favorite(&self, favorite_id: &str) -> Result<(), StorageError> {
        let favorite_id = favorite_id.to_owned();
        self.database.run(move |connection| {
            connection
                .execute("DELETE FROM favorites WHERE id = ?1", params![favorite_id])
                .map_err(|e| classify(e, "删除收藏"))?;
            Ok(())
        })
    }

    fn history_append(&self, entry: &str) -> Result<(), StorageError> {
        let entry = entry.to_owned();
        self.database.run(move |connection| {
            connection.execute("INSERT INTO input_history (entry) VALUES (?1)", params![entry]).map_err(|e| classify(e, "写入历史"))?;
            connection
                .execute("DELETE FROM input_history WHERE id <= (SELECT id FROM input_history ORDER BY id DESC LIMIT 1 OFFSET 199)", [])
                .map_err(|e| classify(e, "裁剪历史"))?;
            Ok(())
        })
    }

    fn history_list(&self) -> Result<Vec<String>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT entry FROM input_history ORDER BY id DESC LIMIT 200")
                .map_err(|e| classify(e, "读取历史"))?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取历史"))?
                .collect::<Result<_, _>>()
                .map_err(|e| classify(e, "读取历史"))?;
            let mut entries: Vec<String> = rows;
            entries.reverse();
            Ok(entries)
        })
    }

    fn history_clear(&self) -> Result<(), StorageError> {
        self.database.run(|connection| {
            connection
                .execute("DELETE FROM input_history", [])
                .map_err(|e| classify(e, "清空历史"))?;
            Ok(())
        })
    }
}

/// 已安装工具存储（installed_tools 表；只有受管安装写入）。
pub struct SqliteToolStore {
    database: Arc<Database>,
}

impl SqliteToolStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl fleqi_application::ports::ToolStore for SqliteToolStore {
    fn upsert(&self, tool: &fleqi_domain::tools::InstalledTool) -> Result<(), StorageError> {
        let json = serde_json::to_string(tool)
            .map_err(|e| StorageError::Io(format!("序列化工具记录失败：{e}")))?;
        let tool_id = tool.manifest.id.clone();
        let installed_at = tool.installed_at.clone();
        let install_dir = tool.install_dir.clone();
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO installed_tools (id, manifest_json, installed_at, install_dir)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET manifest_json = excluded.manifest_json,
                        installed_at = excluded.installed_at, install_dir = excluded.install_dir",
                    params![tool_id, json, installed_at, install_dir],
                )
                .map_err(|e| classify(e, "写入工具记录"))?;
            Ok(())
        })
    }

    fn load_all(&self) -> Result<Vec<fleqi_domain::tools::InstalledTool>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT manifest_json FROM installed_tools ORDER BY installed_at")
                .map_err(|e| classify(e, "读取工具记录"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取工具记录"))?;
            load_json_rows(
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| classify(e, "读取工具记录"))?,
            )
        })
    }

    fn delete(&self, tool_id: &str) -> Result<(), StorageError> {
        let tool_id = tool_id.to_owned();
        self.database.run(move |connection| {
            connection
                .execute(
                    "DELETE FROM installed_tools WHERE id = ?1",
                    params![tool_id],
                )
                .map_err(|e| classify(e, "删除工具记录"))?;
            Ok(())
        })
    }
}

/// 模型端点存储（providers 表；密钥不在此表，走凭据服务）。
pub struct SqliteProviderStore {
    database: Arc<Database>,
}

impl SqliteProviderStore {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl fleqi_application::ports::ProviderStore for SqliteProviderStore {
    fn upsert(
        &self,
        provider: &fleqi_application::provider_service::ProviderRecord,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(provider)
            .map_err(|e| StorageError::Io(format!("序列化端点记录失败：{e}")))?;
        let provider_id = provider.id.clone();
        let stamp = Clock::now_rfc3339(&SystemClock);
        self.database.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO providers (id, json, created_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                    params![provider_id, json, stamp],
                )
                .map_err(|e| classify(e, "写入端点记录"))?;
            Ok(())
        })
    }

    fn load_all(
        &self,
    ) -> Result<Vec<fleqi_application::provider_service::ProviderRecord>, StorageError> {
        self.database.run(|connection| {
            let mut stmt = connection
                .prepare("SELECT json FROM providers ORDER BY created_at")
                .map_err(|e| classify(e, "读取端点记录"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| classify(e, "读取端点记录"))?;
            load_json_rows(
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| classify(e, "读取端点记录"))?,
            )
        })
    }

    fn delete(&self, provider_id: &str) -> Result<(), StorageError> {
        let provider_id = provider_id.to_owned();
        self.database.run(move |connection| {
            connection
                .execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
                .map_err(|e| classify(e, "删除端点记录"))?;
            Ok(())
        })
    }
}

fn load_json_rows<T: serde::de::DeserializeOwned>(
    rows: Vec<String>,
) -> Result<Vec<T>, StorageError> {
    rows.into_iter()
        .map(|json| {
            serde_json::from_str(&json).map_err(|e| StorageError::Corrupt(format!("记录损坏：{e}")))
        })
        .collect()
}
