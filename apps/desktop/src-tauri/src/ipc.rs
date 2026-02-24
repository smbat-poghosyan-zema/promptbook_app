use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

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
pub struct EffortLevelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcModelInfo {
    pub id: String,
    pub name: String,
    pub effort_levels: Vec<EffortLevelInfo>,
    pub default_effort: Option<String>,
    pub is_default: bool,
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
    app_data_dir: PathBuf,
    event_emitter: Arc<dyn RunEventEmitter>,
    model_cache: Arc<Mutex<HashMap<String, Vec<IpcModelInfo>>>>,
}

impl IpcState {
    pub fn new() -> Self {
        Self::with_emitter(Arc::new(NoopRunEventEmitter))
    }

    pub fn with_emitter(event_emitter: Arc<dyn RunEventEmitter>) -> Self {
        Self {
            app_data_dir: Path::new(".").join(".promptbook_runs"),
            event_emitter,
            model_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_emitter_and_data_dir(event_emitter: Arc<dyn RunEventEmitter>, app_data_dir: &Path) -> Self {
        Self {
            app_data_dir: app_data_dir.to_path_buf(),
            event_emitter,
            model_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn resolve_app_data_dir(&self) -> PathBuf {
        self.app_data_dir.clone()
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
    _promptbook_path: Option<String>,
}

impl Default for RunMetadata {
    fn default() -> Self {
        Self { model: None, effort_level: None, workspace_dir: None, _promptbook_path: None }
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
        _promptbook_path: value.get("promptbook_path").and_then(|v| v.as_str()).map(ToOwned::to_owned),
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

// ── Fallback config (embedded at compile time) ────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct AgentModelsEntry {
    models: Vec<IpcModelInfo>,
}

fn load_fallback_models(agent: &str) -> Vec<IpcModelInfo> {
    static CONFIG: OnceLock<HashMap<String, AgentModelsEntry>> = OnceLock::new();
    let config = CONFIG.get_or_init(|| {
        let raw = include_str!("../agent-models.json");
        serde_json::from_str::<HashMap<String, AgentModelsEntry>>(raw).unwrap_or_default()
    });
    config.get(agent).map(|e| e.models.clone()).unwrap_or_default()
}

// ── Per-agent dynamic fetching ────────────────────────────────────────

fn fetch_codex_models() -> IpcResult<Vec<IpcModelInfo>> {
    // Spawn codex app-server with a timeout
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_codex_models_inner());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_else(|_| Err("codex app-server timed out".to_string()))
}

fn fetch_codex_models_inner() -> IpcResult<Vec<IpcModelInfo>> {
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("codex not found: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("failed to open codex stdin")?;
    let stdout = child.stdout.take().ok_or("failed to open codex stdout")?;
    let mut reader = BufReader::new(stdout);

    // 1. Send "initialize"
    send_jsonrpc(&mut stdin, 1, "initialize", json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": { "name": "promptbook-runner", "version": "0.1.0" }
    }))?;
    let _init = read_jsonrpc(&mut reader)?;

    // 2. Send "initialized" notification
    send_jsonrpc_notification(&mut stdin, "notifications/initialized", json!({}))?;

    // 3. Send "model/list"
    send_jsonrpc(&mut stdin, 2, "model/list", json!({ "includeHidden": true }))?;
    let model_response = read_jsonrpc(&mut reader)?;

    let _ = child.kill();
    let _ = child.wait();

    parse_codex_model_list_response(&model_response)
}

fn send_jsonrpc(stdin: &mut impl IoWrite, id: u64, method: &str, params: Value) -> IpcResult<()> {
    let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let body = request.to_string();
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

fn send_jsonrpc_notification(stdin: &mut impl IoWrite, method: &str, params: Value) -> IpcResult<()> {
    let notif = json!({ "jsonrpc": "2.0", "method": method, "params": params });
    let body = notif.to_string();
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

fn read_jsonrpc(reader: &mut impl BufRead) -> IpcResult<Value> {
    let mut content_length: usize = 0;
    let mut header = String::new();
    loop {
        header.clear();
        reader.read_line(&mut header).map_err(|e| e.to_string())?;
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length: ") {
            content_length = val.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        }
    }
    if content_length == 0 {
        return Err("empty Content-Length in JSON-RPC response".to_string());
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn parse_codex_model_list_response(response: &Value) -> IpcResult<Vec<IpcModelInfo>> {
    let models = response
        .get("result")
        .and_then(|r| r.get("models"))
        .and_then(|m| m.as_array())
        .ok_or("missing result.models in codex response")?;

    let mut result = Vec::new();
    for model in models {
        let id = model.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if id.is_empty() { continue; }
        let display_name = model.get("displayName").and_then(|v| v.as_str()).unwrap_or(id);
        let is_default = model.get("isDefault").and_then(|v| v.as_bool()).unwrap_or(false);
        let default_effort = model.get("defaultReasoningEffort").and_then(|v| v.as_str()).map(ToOwned::to_owned);

        let effort_levels: Vec<EffortLevelInfo> = model
            .get("reasoningEffort")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.as_str())
                    .map(|e| EffortLevelInfo { id: e.to_string(), name: effort_display_name(e) })
                    .collect()
            })
            .unwrap_or_default();

        result.push(IpcModelInfo {
            id: id.to_string(),
            name: display_name.to_string(),
            effort_levels,
            default_effort,
            is_default,
        });
    }
    Ok(result)
}

fn effort_display_name(effort_id: &str) -> String {
    match effort_id {
        "low" => "Low".to_string(),
        "medium" => "Medium".to_string(),
        "high" => "High".to_string(),
        "xhigh" => "Extra High".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    }
}

fn fetch_claude_models() -> IpcResult<Vec<IpcModelInfo>> {
    Ok(load_fallback_models("claude"))
}

fn fetch_copilot_models() -> IpcResult<Vec<IpcModelInfo>> {
    let output = Command::new("copilot")
        .args(["--help"])
        .output()
        .map_err(|e| format!("copilot not found: {e}"))?;

    let help_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    parse_copilot_help_models(&help_text)
}

fn parse_copilot_help_models(help_text: &str) -> IpcResult<Vec<IpcModelInfo>> {
    // Look for --model section and extract quoted model names
    // Format: --model <model>  ... (choices: "model-a", "model-b", ...)
    let model_section = help_text.find("--model")
        .ok_or("could not find --model in copilot --help")?;
    let after_model = &help_text[model_section..];

    let mut models = Vec::new();

    // Strategy: find "choices:" or the first quoted string sequence near --model
    // Copilot format: (choices: "claude-sonnet-4.6", "claude-opus-4.6", ...)
    if let Some(choices_start) = after_model.find("choices:") {
        let choices_text = &after_model[choices_start..];
        // Find the closing paren
        let end = choices_text.find(')').unwrap_or(choices_text.len());
        let choices_slice = &choices_text[..end];

        // Extract all quoted strings
        for part in choices_slice.split('"') {
            let trimmed = part.trim();
            // Skip parts that are just commas, whitespace, or "choices:"
            if trimmed.is_empty() || trimmed.starts_with("choices") || trimmed == "," || trimmed == ", " {
                continue;
            }
            // Validate it looks like a model id (contains alphanumeric and dashes/dots)
            if trimmed.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.') && trimmed.contains('-') {
                let is_first = models.is_empty();
                models.push(IpcModelInfo {
                    id: trimmed.to_string(),
                    name: pretty_model_name(trimmed),
                    effort_levels: Vec::new(),
                    default_effort: None,
                    is_default: is_first,
                });
            }
        }
    }

    if models.is_empty() {
        return Err("could not parse model choices from copilot --help".to_string());
    }

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

// ── IPC commands ──────────────────────────────────────────────────────

#[tauri::command]
pub fn list_agent_models(
    state: tauri::State<'_, IpcState>,
    agent: String,
) -> IpcResult<Vec<IpcModelInfo>> {
    // Check cache
    if let Ok(cache) = state.model_cache.lock() {
        if let Some(cached) = cache.get(&agent) {
            return Ok(cached.clone());
        }
    }

    let models = match agent.as_str() {
        "codex"   => fetch_codex_models(),
        "claude"  => fetch_claude_models(),
        "copilot" => fetch_copilot_models(),
        "dry-run" => Ok(Vec::new()),
        _         => Ok(load_fallback_models(&agent)),
    }
    .unwrap_or_else(|_err| load_fallback_models(&agent));

    // Cache the result
    if let Ok(mut cache) = state.model_cache.lock() {
        cache.insert(agent, models.clone());
    }

    Ok(models)
}

#[tauri::command]
pub fn refresh_agent_models(
    state: tauri::State<'_, IpcState>,
    agent: String,
) -> IpcResult<Vec<IpcModelInfo>> {
    if let Ok(mut cache) = state.model_cache.lock() {
        cache.remove(&agent);
    }
    list_agent_models(state, agent)
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
    let app_data_dir = state.resolve_app_data_dir();
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
        &app_data_dir,
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
        effort_display_name, load_fallback_models, parse_codex_model_list_response,
        parse_copilot_help_models, pretty_model_name,
        list_sample_promptbooks, resolve_sample_promptbooks_dir,
        EffortLevelInfo, IpcModelInfo,
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

    #[test]
    fn effort_display_name_maps_correctly() {
        assert_eq!(effort_display_name("low"), "Low");
        assert_eq!(effort_display_name("medium"), "Medium");
        assert_eq!(effort_display_name("high"), "High");
        assert_eq!(effort_display_name("xhigh"), "Extra High");
        assert_eq!(effort_display_name("custom"), "Custom");
    }

    #[test]
    fn fallback_models_loads_codex_entries() {
        let models = load_fallback_models("codex");
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id == "gpt-5.3-codex"));
        let gpt53 = models.iter().find(|m| m.id == "gpt-5.3-codex").unwrap();
        assert!(gpt53.is_default);
        assert!(gpt53.effort_levels.len() >= 3);
        assert_eq!(gpt53.default_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn fallback_models_loads_claude_entries() {
        let models = load_fallback_models("claude");
        assert_eq!(models.len(), 3);
        let haiku = models.iter().find(|m| m.id == "haiku").unwrap();
        assert!(haiku.effort_levels.is_empty());
        assert!(haiku.default_effort.is_none());
        let sonnet = models.iter().find(|m| m.id == "sonnet").unwrap();
        assert!(sonnet.is_default);
        assert!(!sonnet.effort_levels.is_empty());
    }

    #[test]
    fn fallback_models_loads_copilot_entries() {
        let models = load_fallback_models("copilot");
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id == "claude-sonnet-4.6"));
        assert!(models.iter().any(|m| m.id == "gpt-5.3-codex"));
    }

    #[test]
    fn fallback_models_returns_empty_for_unknown_agent() {
        let models = load_fallback_models("unknown-agent");
        assert!(models.is_empty());
    }

    #[test]
    fn fallback_models_dry_run_is_empty() {
        let models = load_fallback_models("dry-run");
        assert!(models.is_empty());
    }

    #[test]
    fn parse_codex_model_list_response_extracts_models() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "models": [
                    {
                        "id": "gpt-5.3-codex",
                        "displayName": "GPT 5.3 Codex",
                        "isDefault": true,
                        "defaultReasoningEffort": "medium",
                        "reasoningEffort": ["low", "medium", "high", "xhigh"]
                    },
                    {
                        "id": "gpt-5.1-codex-mini",
                        "displayName": "GPT 5.1 Codex Mini",
                        "isDefault": false,
                        "defaultReasoningEffort": "medium",
                        "reasoningEffort": ["medium", "high"]
                    }
                ]
            }
        });

        let models = parse_codex_model_list_response(&response).expect("parse codex response");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.3-codex");
        assert!(models[0].is_default);
        assert_eq!(models[0].effort_levels.len(), 4);
        assert_eq!(models[0].effort_levels[3].id, "xhigh");
        assert_eq!(models[0].effort_levels[3].name, "Extra High");
        assert_eq!(models[1].id, "gpt-5.1-codex-mini");
        assert!(!models[1].is_default);
        assert_eq!(models[1].effort_levels.len(), 2);
    }

    #[test]
    fn parse_copilot_help_extracts_model_choices() {
        let help_text = r#"
Usage: copilot [options] [prompt]

Options:
  --model <model>   Set the AI model to use (choices: "claude-sonnet-4.6", "claude-opus-4.6",
                    "gpt-5.3-codex", "gpt-4.1")
  -p, --prompt      The prompt
"#;
        let models = parse_copilot_help_models(help_text).expect("parse copilot help");
        assert_eq!(models.len(), 4);
        assert_eq!(models[0].id, "claude-sonnet-4.6");
        assert!(models[0].is_default);
        assert_eq!(models[1].id, "claude-opus-4.6");
        assert!(!models[1].is_default);
        assert_eq!(models[3].id, "gpt-4.1");
        assert!(models.iter().all(|m| m.effort_levels.is_empty()));
    }

    #[test]
    fn ipc_model_info_json_roundtrip() {
        let model = IpcModelInfo {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            effort_levels: vec![
                EffortLevelInfo { id: "low".to_string(), name: "Low".to_string() },
                EffortLevelInfo { id: "high".to_string(), name: "High".to_string() },
            ],
            default_effort: Some("low".to_string()),
            is_default: true,
        };
        let serialized = serde_json::to_string(&model).expect("serialize");
        let parsed: IpcModelInfo = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed, model);
    }
}
