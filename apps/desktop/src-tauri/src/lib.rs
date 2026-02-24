use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use tauri::Manager;

const DB_FILENAME: &str = "promptbook-runner.sqlite3";
static DB_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub mod process_exec;
pub mod agent_adapter;
pub mod ipc;
pub mod run_manager;

pub use run_manager::{
    cancel_run, run_promptbook, start_run_background, RunEvent, RunEventCallback, RunManagerError,
    RunManagerResult,
};

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
}

impl Display for StorageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(err) => write!(f, "storage IO error: {err}"),
            StorageError::Sql(err) => write!(f, "storage SQLite error: {err}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        StorageError::Io(value)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        StorageError::Sql(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub promptbook_name: String,
    pub promptbook_version: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub agent_default: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub id: i64,
    pub promptbook_name: String,
    pub promptbook_version: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub agent_default: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStep {
    pub run_id: i64,
    pub step_id: String,
    pub title: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub title: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLogLine {
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutput {
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub content: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub content: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDetail {
    pub run: RunRecord,
    pub steps: Vec<StepRecord>,
    pub logs: Vec<LogRecord>,
    pub outputs: Vec<OutputRecord>,
}

pub struct StorageRepository {
    conn: Connection,
}

impl StorageRepository {
    pub fn open_in_app_data_dir(app_data_dir: &Path) -> StorageResult<Self> {
        let db_path = app_data_dir.join(DB_FILENAME);
        Self::open_at(&db_path)
    }

    pub fn open_at(db_path: &Path) -> StorageResult<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Self::migrate(&conn)?;

        Ok(Self { conn })
    }

    fn write_lock() -> &'static Mutex<()> {
        DB_WRITE_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn migrate(conn: &Connection) -> StorageResult<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                promptbook_name TEXT NOT NULL,
                promptbook_version TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                agent_default TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
                step_id TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
                step_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                stream TEXT NOT NULL,
                line TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS outputs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
                step_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                content TEXT NOT NULL,
                format TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id),
                UNIQUE(run_id, step_id)
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_steps_run_id ON steps(run_id);
            CREATE INDEX IF NOT EXISTS idx_logs_run_id ON logs(run_id);
            CREATE INDEX IF NOT EXISTS idx_outputs_run_id ON outputs(run_id);
            ",
        )?;

        Ok(())
    }

    pub fn create_run(&self, new_run: &NewRun) -> StorageResult<i64> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            INSERT INTO runs
            (promptbook_name, promptbook_version, status, started_at, finished_at, agent_default, metadata_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                &new_run.promptbook_name,
                &new_run.promptbook_version,
                &new_run.status,
                &new_run.started_at,
                &new_run.finished_at,
                &new_run.agent_default,
                &new_run.metadata_json
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_run_status(
        &self,
        run_id: i64,
        status: &str,
        finished_at: Option<&str>,
    ) -> StorageResult<()> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            UPDATE runs
            SET status = ?1,
                finished_at = COALESCE(?2, finished_at)
            WHERE id = ?3
            ",
            params![status, finished_at, run_id],
        )?;

        Ok(())
    }

    pub fn create_step(&self, new_step: &NewStep) -> StorageResult<i64> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            INSERT INTO steps
            (run_id, step_id, title, status, started_at, finished_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                new_step.run_id,
                &new_step.step_id,
                &new_step.title,
                &new_step.status,
                &new_step.started_at,
                &new_step.finished_at
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_step_status(
        &self,
        run_id: i64,
        step_id: &str,
        status: &str,
        finished_at: Option<&str>,
    ) -> StorageResult<()> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            UPDATE steps
            SET status = ?1,
                finished_at = COALESCE(?2, finished_at)
            WHERE run_id = ?3 AND step_id = ?4
            ",
            params![status, finished_at, run_id, step_id],
        )?;

        Ok(())
    }

    pub fn append_log_line(&self, log: &NewLogLine) -> StorageResult<i64> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            INSERT INTO logs
            (run_id, step_id, ts, stream, line)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![log.run_id, &log.step_id, &log.ts, &log.stream, &log.line],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_step_output(&self, output: &StepOutput) -> StorageResult<()> {
        let _guard = Self::write_lock()
            .lock()
            .map_err(|_| StorageError::Sql(rusqlite::Error::InvalidQuery))?;
        self.conn.execute(
            "
            INSERT INTO outputs
            (run_id, step_id, ts, content, format)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(run_id, step_id) DO UPDATE
            SET ts = excluded.ts,
                content = excluded.content,
                format = excluded.format
            ",
            params![
                output.run_id,
                &output.step_id,
                &output.ts,
                &output.content,
                &output.format
            ],
        )?;

        Ok(())
    }

    pub fn get_setting_value_json(&self, key: &str) -> StorageResult<Option<String>> {
        self.conn
            .query_row(
                "
                SELECT value_json
                FROM settings
                WHERE key = ?1
                ",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_runs(&self) -> StorageResult<Vec<RunRecord>> {
        let mut stmt = self.conn.prepare(
            "
            SELECT id, promptbook_name, promptbook_version, status, started_at, finished_at, agent_default, metadata_json
            FROM runs
            ORDER BY started_at DESC, id DESC
            ",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(RunRecord {
                id: row.get(0)?,
                promptbook_name: row.get(1)?,
                promptbook_version: row.get(2)?,
                status: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                agent_default: row.get(6)?,
                metadata_json: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn get_run_detail(&self, run_id: i64) -> StorageResult<Option<RunDetail>> {
        let run = self
            .conn
            .query_row(
                "
                SELECT id, promptbook_name, promptbook_version, status, started_at, finished_at, agent_default, metadata_json
                FROM runs
                WHERE id = ?1
                ",
                [run_id],
                |row| {
                    Ok(RunRecord {
                        id: row.get(0)?,
                        promptbook_name: row.get(1)?,
                        promptbook_version: row.get(2)?,
                        status: row.get(3)?,
                        started_at: row.get(4)?,
                        finished_at: row.get(5)?,
                        agent_default: row.get(6)?,
                        metadata_json: row.get(7)?,
                    })
                },
            )
            .optional()?;

        let Some(run) = run else {
            return Ok(None);
        };

        let mut steps_stmt = self.conn.prepare(
            "
            SELECT id, run_id, step_id, title, status, started_at, finished_at
            FROM steps
            WHERE run_id = ?1
            ORDER BY id ASC
            ",
        )?;
        let steps = steps_stmt
            .query_map([run_id], |row| {
                Ok(StepRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step_id: row.get(2)?,
                    title: row.get(3)?,
                    status: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut logs_stmt = self.conn.prepare(
            "
            SELECT id, run_id, step_id, ts, stream, line
            FROM logs
            WHERE run_id = ?1
            ORDER BY id ASC
            ",
        )?;
        let logs = logs_stmt
            .query_map([run_id], |row| {
                Ok(LogRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step_id: row.get(2)?,
                    ts: row.get(3)?,
                    stream: row.get(4)?,
                    line: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut outputs_stmt = self.conn.prepare(
            "
            SELECT id, run_id, step_id, ts, content, format
            FROM outputs
            WHERE run_id = ?1
            ORDER BY id ASC
            ",
        )?;
        let outputs = outputs_stmt
            .query_map([run_id], |row| {
                Ok(OutputRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step_id: row.get(2)?,
                    ts: row.get(3)?,
                    content: row.get(4)?,
                    format: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(RunDetail {
            run,
            steps,
            logs,
            outputs,
        }))
    }
}

pub fn placeholder_engine_value(value: i32) -> i32 {
    value + 1
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".promptbook_runs"));
            let emitter = std::sync::Arc::new(ipc::TauriRunEventEmitter::new(app.handle().clone()));
            let state = ipc::IpcState::with_emitter_and_data_dir(emitter, &app_data_dir);
            app.manage(state);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            ipc::list_runs,
            ipc::get_run_detail,
            ipc::start_run,
            ipc::cancel_run,
            ipc::open_file_picker_for_promptbook,
            ipc::open_sample_promptbooks_folder,
            ipc::list_sample_promptbooks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        placeholder_engine_value, NewLogLine, NewRun, NewStep, StepOutput, StorageRepository,
    };

    fn temp_db_path(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time moved backwards")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "promptbook-runner-{test_name}-{}-{nanos}.sqlite3",
            std::process::id()
        ))
    }

    #[test]
    fn create_run_and_list_runs_returns_inserted_run() {
        let db_path = temp_db_path("create-run-list");
        let repo = StorageRepository::open_at(&db_path).expect("open db");

        let run_id = repo
            .create_run(&NewRun {
                promptbook_name: "hello-world".to_string(),
                promptbook_version: "v1".to_string(),
                status: "running".to_string(),
                started_at: "2026-02-21T00:00:00Z".to_string(),
                finished_at: None,
                agent_default: Some("codex".to_string()),
                metadata_json: Some("{\"source\":\"test\"}".to_string()),
            })
            .expect("create run");

        let runs = repo.list_runs().expect("list runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, run_id);
        assert_eq!(runs[0].promptbook_name, "hello-world");
        assert_eq!(runs[0].status, "running");

        drop(repo);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn append_logs_set_output_and_retrieve_detail() {
        let db_path = temp_db_path("run-detail");
        let repo = StorageRepository::open_at(&db_path).expect("open db");

        let run_id = repo
            .create_run(&NewRun {
                promptbook_name: "hello-world".to_string(),
                promptbook_version: "v1".to_string(),
                status: "running".to_string(),
                started_at: "2026-02-21T01:00:00Z".to_string(),
                finished_at: None,
                agent_default: Some("codex".to_string()),
                metadata_json: None,
            })
            .expect("create run");

        let _step_row_id = repo
            .create_step(&NewStep {
                run_id,
                step_id: "step-1".to_string(),
                title: "Generate output".to_string(),
                status: "running".to_string(),
                started_at: Some("2026-02-21T01:01:00Z".to_string()),
                finished_at: None,
            })
            .expect("create step");

        repo.append_log_line(&NewLogLine {
            run_id,
            step_id: "step-1".to_string(),
            ts: "2026-02-21T01:01:01Z".to_string(),
            stream: "stdout".to_string(),
            line: "line 1".to_string(),
        })
        .expect("append first log line");
        repo.append_log_line(&NewLogLine {
            run_id,
            step_id: "step-1".to_string(),
            ts: "2026-02-21T01:01:02Z".to_string(),
            stream: "stdout".to_string(),
            line: "line 2".to_string(),
        })
        .expect("append second log line");

        repo.set_step_output(&StepOutput {
            run_id,
            step_id: "step-1".to_string(),
            ts: "2026-02-21T01:01:03Z".to_string(),
            content: "{\"ok\":true}".to_string(),
            format: "json".to_string(),
        })
        .expect("set step output");

        let detail = repo
            .get_run_detail(run_id)
            .expect("get run detail")
            .expect("run detail missing");

        assert_eq!(detail.run.id, run_id);
        assert_eq!(detail.steps.len(), 1);
        assert_eq!(detail.logs.len(), 2);
        assert_eq!(detail.logs[0].line, "line 1");
        assert_eq!(detail.outputs.len(), 1);
        assert_eq!(detail.outputs[0].format, "json");
        assert_eq!(detail.outputs[0].content, "{\"ok\":true}");

        drop(repo);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn increments_placeholder_value() {
        assert_eq!(placeholder_engine_value(41), 42);
    }
}
