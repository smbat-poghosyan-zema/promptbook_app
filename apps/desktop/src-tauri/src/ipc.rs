use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{
    cancel_run as cancel_run_sync, resume_run_in_place, start_run_background, LogRecord,
    OutputRecord, RunDetail, RunEvent, RunEventCallback, RunRecord, StepRecord, StorageRepository,
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
    pub model: Option<String>,
    pub effort_level: Option<String>,
    pub workspace_dir: Option<String>,
    pub step_count: i64,
    pub current_step_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcModelInfo {
    pub id: String,
    pub name: String,
    pub supports_effort: bool,
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
    pub prompt: Option<String>,
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
pub struct TauriRunEventEmitter {
    app_handle: AppHandle,
}

impl TauriRunEventEmitter {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl RunEventEmitter for TauriRunEventEmitter {
    fn emit_run_event(&self, event: RunEventEnvelope) -> IpcResult<()> {
        self.app_handle
            .emit(RUN_EVENT_NAME, &event)
            .map_err(|err| err.to_string())
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

    pub fn with_emitter_and_data_dir(event_emitter: Arc<dyn RunEventEmitter>, app_data_dir: &Path) -> Self {
        Self {
            app_data_dir: Arc::new(Mutex::new(Some(app_data_dir.to_path_buf()))),
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

struct RunMetadata {
    model: Option<String>,
    effort_level: Option<String>,
    workspace_dir: Option<String>,
    promptbook_path: Option<String>,
}

impl Default for RunMetadata {
    fn default() -> Self {
        Self { model: None, effort_level: None, workspace_dir: None, promptbook_path: None }
    }
}

fn parse_run_metadata(metadata: Option<&str>) -> RunMetadata {
    let Some(json) = metadata else { return RunMetadata::default() };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return RunMetadata::default();
    };
    RunMetadata {
        model: value.get("model").and_then(|v| v.as_str()).map(ToOwned::to_owned),
        effort_level: value.get("effort_level").and_then(|v| v.as_str()).map(ToOwned::to_owned),
        workspace_dir: value.get("workspace_dir").and_then(|v| v.as_str()).map(ToOwned::to_owned),
        promptbook_path: value.get("promptbook_path").and_then(|v| v.as_str()).map(ToOwned::to_owned),
    }
}

impl From<RunRecord> for IpcRunRecord {
    fn from(value: RunRecord) -> Self {
        let meta = parse_run_metadata(value.metadata_json.as_deref());
        Self {
            id: value.id,
            promptbook_name: value.promptbook_name,
            promptbook_version: value.promptbook_version,
            status: value.status,
            started_at: value.started_at,
            finished_at: value.finished_at,
            agent_default: value.agent_default,
            metadata_json: value.metadata_json,
            model: meta.model,
            effort_level: meta.effort_level,
            workspace_dir: meta.workspace_dir,
            step_count: value.step_count,
            current_step_title: value.current_step_title,
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
            prompt: value.prompt,
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

#[tauri::command]
pub fn list_runs(state: tauri::State<'_, IpcState>) -> IpcResult<Vec<IpcRunRecord>> {
    let repo = StorageRepository::open_in_app_data_dir(&state.resolve_app_data_dir())
        .map_err(|err| err.to_string())?;
    let runs = repo.list_runs().map_err(|err| err.to_string())?;
    Ok(runs.into_iter().map(IpcRunRecord::from).collect())
}

#[tauri::command]
pub fn get_run_detail(state: tauri::State<'_, IpcState>, run_id: i64) -> IpcResult<Option<IpcRunDetail>> {
    let repo = StorageRepository::open_in_app_data_dir(&state.resolve_app_data_dir())
        .map_err(|err| err.to_string())?;
    let detail = repo
        .get_run_detail(run_id)
        .map_err(|err| err.to_string())?;
    Ok(detail.map(IpcRunDetail::from))
}

#[tauri::command]
pub fn list_agent_models(agent: String) -> IpcResult<Vec<IpcModelInfo>> {
    let output = std::process::Command::new("openclaw")
        .args(["models", "--status-json"])
        .output()
        .map_err(|err| format!("openclaw not found: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "openclaw models failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse openclaw output: {err}"))?;

    let allowed = json
        .get("allowed")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "openclaw output missing 'allowed' field".to_string())?;

    let models: Vec<IpcModelInfo> = allowed
        .iter()
        .filter_map(|v| v.as_str())
        .filter_map(|full_id| {
            let (provider, model_id) = full_id.split_once('/')?;
            let matches_agent = match agent.as_str() {
                "claude" => provider == "anthropic",
                "codex"  => provider == "openai" || provider == "openai-codex",
                _        => false,
            };
            if !matches_agent {
                return None;
            }
            let supports_effort = matches!(provider, "anthropic" | "openai" | "openai-codex");
            let name = pretty_model_name(model_id);
            Some(IpcModelInfo {
                id: model_id.to_string(),
                name,
                supports_effort,
            })
        })
        // Deduplicate by id (openai/ and openai-codex/ may repeat same model)
        .fold(Vec::new(), |mut acc, m| {
            if !acc.iter().any(|x: &IpcModelInfo| x.id == m.id) {
                acc.push(m);
            }
            acc
        });

    Ok(models)
}

fn pretty_model_name(model_id: &str) -> String {
    model_id
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    if first.is_ascii_digit() {
                        part.to_string()
                    } else {
                        let upper = first.to_uppercase().collect::<String>();
                        upper + chars.as_str()
                    }
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
pub fn start_run(
    state: tauri::State<'_, IpcState>,
    promptbook_path: String,
    agent: Option<String>,
    model: Option<String>,
    effort_level: Option<String>,
    workspace_dir: String,
) -> IpcResult<i64> {
    state.set_workspace_dir(&workspace_dir);
    let emitter = Arc::clone(&state.event_emitter);
    let callback = Arc::new(move |event: RunEvent| {
        let _ = emitter.emit_run_event(map_run_event(event));
    });
    start_run_background(
        &promptbook_path,
        agent.as_deref(),
        &workspace_dir,
        model.as_deref(),
        effort_level.as_deref(),
        Some(callback),
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cancel_run(run_id: i64) -> IpcResult<bool> {
    cancel_run_sync(run_id).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn resume_run(
    state: tauri::State<'_, IpcState>,
    original_run_id: i64,
) -> IpcResult<i64> {
    let app_data_dir = state.resolve_app_data_dir();
    let emitter = Arc::clone(&state.event_emitter);
    let callback: RunEventCallback = Arc::new(move |event: RunEvent| {
        let _ = emitter.emit_run_event(map_run_event(event));
    });

    resume_run_in_place(original_run_id, &app_data_dir, Some(callback))
        .map_err(|err| err.to_string())?;

    // Return the SAME run_id so the UI re-selects it
    Ok(original_run_id)
}

#[tauri::command]
pub async fn open_file_picker_for_promptbook(
    app_handle: tauri::AppHandle,
) -> IpcResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let path = app_handle
        .dialog()
        .file()
        .add_filter("Promptbook YAML", &["yaml", "yml"])
        .blocking_pick_file();
    Ok(path.map(|p| p.to_string()))
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
        for dir_name in &["promptbooks", "sample-promptbooks"] {
            let candidate = ancestor.join(dir_name);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }

    Err("promptbooks directory not found — set PROMPTBOOK_SAMPLE_DIR or create a 'promptbooks' folder".to_string())
}

#[tauri::command]
pub async fn open_sample_promptbooks_folder(
    app_handle: tauri::AppHandle,
) -> IpcResult<String> {
    use tauri_plugin_shell::ShellExt;
    let sample_dir = resolve_sample_promptbooks_dir()?;
    let path_str = sample_dir.to_string_lossy().to_string();
    // Open in system file manager (xdg-open on Linux)
    app_handle
        .shell()
        .command("xdg-open")
        .args([&path_str])
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(path_str)
}

#[tauri::command]
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
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        list_sample_promptbooks, pretty_model_name, resolve_sample_promptbooks_dir,
        IpcRunDetail, IpcRunRecord, RunEventEnvelope, RunEventType, RUN_EVENT_NAME,
    };
    use serde_json::json;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[derive(Debug)]
    struct EnvVarGuard {
        key: &'static str,
        previous_value: Option<String>,
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous_value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn set_env_var(key: &'static str, value: Option<&str>) -> EnvVarGuard {
        let previous_value = std::env::var(key).ok();
        match value {
            Some(new_value) => std::env::set_var(key, new_value),
            None => std::env::remove_var(key),
        }
        EnvVarGuard {
            key,
            previous_value,
        }
    }

    fn temp_dir(test_name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time moved backwards")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "promptbook-ipc-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create temp dir");
        directory
    }

    // open_file_picker_for_promptbook now uses tauri_plugin_dialog and requires
    // a live AppHandle; it cannot be tested in unit tests.

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
                model: None,
                effort_level: None,
                workspace_dir: None,
                step_count: 0,
                current_step_title: None,
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
    fn run_event_name_is_correct() {
        assert_eq!(RUN_EVENT_NAME, "run_event");
    }

    #[test]
    fn sample_promptbook_listing_filters_and_sorts_v1_yaml_files() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = temp_dir("sample-list");
        fs::write(temp.join("repo-audit.v1.yaml"), "schema_version: \"promptbook/v1\"")
            .expect("write sample");
        fs::write(temp.join("hello-world.v1.yaml"), "schema_version: \"promptbook/v1\"")
            .expect("write sample");
        fs::write(temp.join("ignore.txt"), "ignore").expect("write noise file");

        let _sample_guard = set_env_var("PROMPTBOOK_SAMPLE_DIR", Some(&temp.to_string_lossy()));

        let folder = resolve_sample_promptbooks_dir().expect("resolve sample folder");
        assert_eq!(folder.to_string_lossy(), temp.to_string_lossy());

        let samples = list_sample_promptbooks().expect("list samples");
        let sample_ids = samples.iter().map(|sample| sample.id.as_str()).collect::<Vec<_>>();
        assert_eq!(sample_ids, vec!["hello-world", "repo-audit"]);
        assert!(samples.iter().all(|sample| sample.path.ends_with(".v1.yaml")));

        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn pretty_model_name_formats_correctly() {
        assert_eq!(pretty_model_name("claude-sonnet-4-6"), "Claude Sonnet 4 6");
        assert_eq!(pretty_model_name("gpt-5.3-codex-spark"), "Gpt 5.3 Codex Spark");
        assert_eq!(pretty_model_name("claude-opus-4-6"), "Claude Opus 4 6");
    }
}
