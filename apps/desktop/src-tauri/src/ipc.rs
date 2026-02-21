use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    cancel_run as cancel_run_sync, start_run_background, LogRecord, OutputRecord, RunDetail,
    RunEvent, RunRecord, StepRecord, StorageRepository,
};

pub type IpcResult<T> = Result<T, String>;
pub const RUN_EVENT_NAME: &str = "run_event";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcRunRecord {
    pub id: i64,
    pub promptbook_name: String,
    pub promptbook_version: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub agent_default: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcStepRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub title: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcLogRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcOutputRecord {
    pub id: i64,
    pub run_id: i64,
    pub step_id: String,
    pub ts: String,
    pub content: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcRunDetail {
    pub run: IpcRunRecord,
    pub steps: Vec<IpcStepRecord>,
    pub logs: Vec<IpcLogRecord>,
    pub outputs: Vec<IpcOutputRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunEventType {
    StepStarted,
    StepProgressLine,
    StepFinished,
    RunFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunEventEnvelope {
    pub run_id: i64,
    pub step_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: RunEventType,
    pub payload: Value,
}

pub trait RunEventEmitter: Send + Sync {
    fn emit_run_event(&self, event: RunEventEnvelope) -> IpcResult<()>;
}

#[derive(Clone, Default)]
pub struct NoopRunEventEmitter;

impl RunEventEmitter for NoopRunEventEmitter {
    fn emit_run_event(&self, _event: RunEventEnvelope) -> IpcResult<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct IpcState {
    app_data_dir: Arc<Mutex<Option<PathBuf>>>,
    event_emitter: Arc<dyn RunEventEmitter>,
}

impl IpcState {
    pub fn new() -> Self {
        Self::with_emitter(Arc::new(NoopRunEventEmitter))
    }

    pub fn with_emitter(event_emitter: Arc<dyn RunEventEmitter>) -> Self {
        Self {
            app_data_dir: Arc::new(Mutex::new(None)),
            event_emitter,
        }
    }

    fn resolve_app_data_dir(&self) -> PathBuf {
        let guard = self.app_data_dir.lock();
        if let Ok(state) = guard {
            if let Some(path) = state.clone() {
                return path;
            }
        }
        Path::new(".").join(".promptbook_runs")
    }

    fn set_workspace_dir(&self, workspace_dir: &str) {
        if let Ok(mut state) = self.app_data_dir.lock() {
            *state = Some(Path::new(workspace_dir).join(".promptbook_runs"));
        }
    }
}

impl Default for IpcState {
    fn default() -> Self {
        Self::new()
    }
}

impl From<RunRecord> for IpcRunRecord {
    fn from(value: RunRecord) -> Self {
        Self {
            id: value.id,
            promptbook_name: value.promptbook_name,
            promptbook_version: value.promptbook_version,
            status: value.status,
            started_at: value.started_at,
            finished_at: value.finished_at,
            agent_default: value.agent_default,
            metadata_json: value.metadata_json,
        }
    }
}

impl From<StepRecord> for IpcStepRecord {
    fn from(value: StepRecord) -> Self {
        Self {
            id: value.id,
            run_id: value.run_id,
            step_id: value.step_id,
            title: value.title,
            status: value.status,
            started_at: value.started_at,
            finished_at: value.finished_at,
        }
    }
}

impl From<LogRecord> for IpcLogRecord {
    fn from(value: LogRecord) -> Self {
        Self {
            id: value.id,
            run_id: value.run_id,
            step_id: value.step_id,
            ts: value.ts,
            stream: value.stream,
            line: value.line,
        }
    }
}

impl From<OutputRecord> for IpcOutputRecord {
    fn from(value: OutputRecord) -> Self {
        Self {
            id: value.id,
            run_id: value.run_id,
            step_id: value.step_id,
            ts: value.ts,
            content: value.content,
            format: value.format,
        }
    }
}

impl From<RunDetail> for IpcRunDetail {
    fn from(value: RunDetail) -> Self {
        Self {
            run: value.run.into(),
            steps: value.steps.into_iter().map(IpcStepRecord::from).collect(),
            logs: value.logs.into_iter().map(IpcLogRecord::from).collect(),
            outputs: value
                .outputs
                .into_iter()
                .map(IpcOutputRecord::from)
                .collect(),
        }
    }
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn list_runs(state: &IpcState) -> IpcResult<Vec<IpcRunRecord>> {
    let repo = StorageRepository::open_in_app_data_dir(&state.resolve_app_data_dir())
        .map_err(|err| err.to_string())?;
    let runs = repo.list_runs().map_err(|err| err.to_string())?;
    Ok(runs.into_iter().map(IpcRunRecord::from).collect())
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn get_run_detail(state: &IpcState, run_id: i64) -> IpcResult<Option<IpcRunDetail>> {
    let repo = StorageRepository::open_in_app_data_dir(&state.resolve_app_data_dir())
        .map_err(|err| err.to_string())?;
    let detail = repo
        .get_run_detail(run_id)
        .map_err(|err| err.to_string())?;
    Ok(detail.map(IpcRunDetail::from))
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn start_run(
    state: &IpcState,
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
) -> IpcResult<i64> {
    state.set_workspace_dir(workspace_dir);
    let emitter = Arc::clone(&state.event_emitter);
    let callback = Arc::new(move |event: RunEvent| {
        let _ = emitter.emit_run_event(map_run_event(event));
    });
    start_run_background(promptbook_path, agent, workspace_dir, Some(callback))
        .map_err(|err| err.to_string())
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn cancel_run(run_id: i64) -> IpcResult<bool> {
    cancel_run_sync(run_id).map_err(|err| err.to_string())
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn open_file_picker_for_promptbook() -> IpcResult<Option<String>> {
    let configured = std::env::var("PROMPTBOOK_PATH").ok();
    let Some(path) = configured else {
        return Ok(None);
    };
    if Path::new(&path).exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcSamplePromptbook {
    pub id: String,
    pub title: String,
    pub path: String,
}

fn resolve_sample_promptbooks_dir() -> IpcResult<PathBuf> {
    if let Ok(configured) = std::env::var("PROMPTBOOK_SAMPLE_DIR") {
        let configured_path = PathBuf::from(configured);
        if configured_path.is_dir() {
            return Ok(configured_path);
        }
    }

    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join("sample-promptbooks");
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err("sample-promptbooks directory not found".to_string())
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn open_sample_promptbooks_folder() -> IpcResult<String> {
    let sample_dir = resolve_sample_promptbooks_dir()?;
    Ok(sample_dir.to_string_lossy().to_string())
}

#[cfg_attr(feature = "tauri", tauri::command)]
pub fn list_sample_promptbooks() -> IpcResult<Vec<IpcSamplePromptbook>> {
    let sample_dir = resolve_sample_promptbooks_dir()?;
    let mut samples: Vec<IpcSamplePromptbook> = std::fs::read_dir(&sample_dir)
        .map_err(|err| err.to_string())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.ends_with(".v1.yaml"))
        })
        .filter_map(|path| {
            let file_name = path.file_name()?.to_str()?.to_string();
            let id = file_name.trim_end_matches(".v1.yaml").to_string();
            let title = id.replace('-', " ");
            Some(IpcSamplePromptbook {
                id,
                title,
                path: path.to_string_lossy().to_string(),
            })
        })
        .collect();

    samples.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(samples)
}

fn map_run_event(event: RunEvent) -> RunEventEnvelope {
    match event {
        RunEvent::StepStarted {
            run_id,
            step_id,
            title,
            ts,
        } => RunEventEnvelope {
            run_id,
            step_id: Some(step_id),
            event_type: RunEventType::StepStarted,
            payload: json!({
                "title": title,
                "ts": ts
            }),
        },
        RunEvent::StepProgressLine {
            run_id,
            step_id,
            stream,
            line,
            ts,
        } => RunEventEnvelope {
            run_id,
            step_id: Some(step_id),
            event_type: RunEventType::StepProgressLine,
            payload: json!({
                "stream": stream,
                "line": line,
                "ts": ts
            }),
        },
        RunEvent::StepFinished {
            run_id,
            step_id,
            status,
            ts,
        } => RunEventEnvelope {
            run_id,
            step_id: Some(step_id),
            event_type: RunEventType::StepFinished,
            payload: json!({
                "status": status,
                "ts": ts
            }),
        },
        RunEvent::RunFinished { run_id, status, ts } => RunEventEnvelope {
            run_id,
            step_id: None,
            event_type: RunEventType::RunFinished,
            payload: json!({
                "status": status,
                "ts": ts
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_run, get_run_detail, list_runs, list_sample_promptbooks,
        open_file_picker_for_promptbook, open_sample_promptbooks_folder, start_run, IpcResult,
        IpcRunDetail, IpcRunRecord, IpcSamplePromptbook, IpcState, RunEventEnvelope, RunEventType,
        RUN_EVENT_NAME,
    };
    use serde_json::json;

    #[test]
    fn run_event_envelope_json_roundtrip() {
        let event = RunEventEnvelope {
            run_id: 42,
            step_id: Some("step-1".to_string()),
            event_type: RunEventType::StepProgressLine,
            payload: json!({
                "stream": "stdout",
                "line": "working",
                "ts": "1700000000.000Z"
            }),
        };

        let serialized = serde_json::to_string(&event).expect("serialize");
        let parsed: RunEventEnvelope = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed, event);
    }

    #[test]
    fn run_payload_structs_json_roundtrip() {
        let detail = IpcRunDetail {
            run: IpcRunRecord {
                id: 7,
                promptbook_name: "demo".to_string(),
                promptbook_version: "1.0.0".to_string(),
                status: "running".to_string(),
                started_at: "1700000000.000Z".to_string(),
                finished_at: None,
                agent_default: Some("codex".to_string()),
                metadata_json: Some("{\"k\":\"v\"}".to_string()),
            },
            steps: Vec::new(),
            logs: Vec::new(),
            outputs: Vec::new(),
        };

        let serialized = serde_json::to_string(&detail).expect("serialize");
        let parsed: IpcRunDetail = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed, detail);
    }

    #[test]
    fn command_signatures_compile() {
        let _list_runs: fn(&IpcState) -> IpcResult<Vec<IpcRunRecord>> = list_runs;
        let _get_run_detail: fn(&IpcState, i64) -> IpcResult<Option<IpcRunDetail>> = get_run_detail;
        let _start_run: fn(&IpcState, &str, Option<&str>, &str) -> IpcResult<i64> = start_run;
        let _cancel_run: fn(i64) -> IpcResult<bool> = cancel_run;
        let _picker: fn() -> IpcResult<Option<String>> = open_file_picker_for_promptbook;
        let _open_samples_folder: fn() -> IpcResult<String> = open_sample_promptbooks_folder;
        let _sample_list: fn() -> IpcResult<Vec<IpcSamplePromptbook>> = list_sample_promptbooks;
        assert_eq!(RUN_EVENT_NAME, "run_event");
    }
}
