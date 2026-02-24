use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime as TokioRuntime};
use tokio::task::JoinHandle as TokioJoinHandle;

use crate::agent_adapter::{
    AdapterOptions, AgentAdapter, ClaudeAdapter, CodexAdapter, CopilotAdapter, DryRunAdapter,
};
use crate::process_exec::{spawn_process, OutputStream, ProcessHandle, ProcessOptions};
use crate::{NewLogLine, NewRun, NewStep, StepOutput, StorageError, StorageRepository};

const DEFAULT_MAX_PARALLEL_RUNS: usize = 2;
const MAX_PARALLEL_RUNS_SETTING_KEY: &str = "max_parallel_runs";

pub type RunManagerResult<T> = Result<T, RunManagerError>;

#[derive(Debug)]
pub enum RunManagerError {
    Io(std::io::Error),
    Storage(StorageError),
    PromptbookParse(String),
    UnknownAgent(String),
    ProcessSpawn { program: String, source: std::io::Error },
    ProcessWait(std::io::Error),
    ActiveRunState(String),
}

impl Display for RunManagerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RunManagerError::Io(err) => write!(f, "run manager IO error: {err}"),
            RunManagerError::Storage(err) => write!(f, "run manager storage error: {err}"),
            RunManagerError::PromptbookParse(err) => {
                write!(f, "failed to parse promptbook: {err}")
            }
            RunManagerError::UnknownAgent(agent) => write!(f, "unknown agent adapter: {agent}"),
            RunManagerError::ProcessSpawn { program, source } => {
                write!(f, "failed to spawn process `{program}`: {source}")
            }
            RunManagerError::ProcessWait(err) => write!(f, "failed while waiting for process: {err}"),
            RunManagerError::ActiveRunState(err) => write!(f, "active run state error: {err}"),
        }
    }
}

impl Error for RunManagerError {}

impl From<std::io::Error> for RunManagerError {
    fn from(value: std::io::Error) -> Self {
        RunManagerError::Io(value)
    }
}

impl From<StorageError> for RunManagerError {
    fn from(value: StorageError) -> Self {
        RunManagerError::Storage(value)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PromptbookFile {
    schema_version: String,
    name: String,
    version: String,
    #[serde(default)]
    defaults: PromptbookDefaults,
    #[serde(default)]
    continue_on_error: Option<bool>,
    steps: Vec<PromptbookStep>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PromptbookDefaults {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    continue_on_error: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromptbookStep {
    id: String,
    title: String,
    prompt: String,
    #[serde(default)]
    verify: Vec<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    continue_on_error: Option<bool>,
}

#[derive(Debug)]
struct RunHandle {
    cancel_requested: Arc<AtomicBool>,
    current_process: Option<Arc<Mutex<ProcessHandle>>>,
    task: Option<TokioJoinHandle<()>>,
}

#[derive(Debug)]
struct ParallelRunLimiter {
    state: Mutex<ParallelRunLimiterState>,
    condvar: Condvar,
}

#[derive(Debug, Clone, Copy)]
struct ParallelRunLimiterState {
    max_parallel_runs: usize,
    active_runs: usize,
}

impl ParallelRunLimiter {
    fn new() -> Self {
        Self {
            state: Mutex::new(ParallelRunLimiterState {
                max_parallel_runs: DEFAULT_MAX_PARALLEL_RUNS,
                active_runs: 0,
            }),
            condvar: Condvar::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    StepStarted {
        run_id: i64,
        step_id: String,
        title: String,
        ts: String,
    },
    StepProgressLine {
        run_id: i64,
        step_id: String,
        stream: String,
        line: String,
        ts: String,
    },
    StepFinished {
        run_id: i64,
        step_id: String,
        status: String,
        ts: String,
    },
    RunFinished {
        run_id: i64,
        status: String,
        ts: String,
    },
}

pub type RunEventCallback = Arc<dyn Fn(RunEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone)]
struct PreparedRun {
    run_id: i64,
    promptbook: PromptbookFile,
    selected_agent: Option<String>,
    selected_model: Option<String>,
    selected_effort: Option<String>,
    workspace_path: PathBuf,
    app_data_dir: PathBuf,
    max_parallel_runs: usize,
    from_step_id: Option<String>,
}

static RUN_HANDLES: OnceLock<Mutex<HashMap<i64, RunHandle>>> = OnceLock::new();
static RUN_RUNTIME: OnceLock<TokioRuntime> = OnceLock::new();
static PARALLEL_RUN_LIMITER: OnceLock<ParallelRunLimiter> = OnceLock::new();

pub fn run_promptbook(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
    model: Option<&str>,
    effort_level: Option<&str>,
) -> RunManagerResult<i64> {
    let prepared = prepare_run(promptbook_path, agent, workspace_dir, model, effort_level)?;
    let run_id = prepared.run_id;
    register_run_handle(run_id)?;
    if let Err(err) = execute_prepared_run_with_limits(prepared, None) {
        let _ = unregister_run_handle(run_id);
        return Err(err);
    }
    Ok(run_id)
}

pub fn start_run_background(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
    model: Option<&str>,
    effort_level: Option<&str>,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<i64> {
    let prepared = prepare_run(promptbook_path, agent, workspace_dir, model, effort_level)?;
    let run_id = prepared.run_id;
    register_run_handle(run_id)?;
    let app_data_dir = prepared.app_data_dir.clone();
    let join_handle = run_runtime().spawn_blocking(move || {
        if execute_prepared_run_with_limits(prepared, event_callback).is_err() {
            let _ = unregister_run_handle(run_id);
            if let Ok(repo) = StorageRepository::open_in_app_data_dir(&app_data_dir) {
                let _ = repo.update_run_status(run_id, "failure", Some(&now_timestamp()));
            }
        }
    });
    set_run_task(run_id, join_handle)?;

    Ok(run_id)
}

pub fn start_run_background_from(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
    model: Option<&str>,
    effort_level: Option<&str>,
    from_step_id: Option<&str>,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<i64> {
    let mut prepared = prepare_run(promptbook_path, agent, workspace_dir, model, effort_level)?;
    prepared.from_step_id = from_step_id.map(ToOwned::to_owned);
    let run_id = prepared.run_id;
    register_run_handle(run_id)?;
    let app_data_dir = prepared.app_data_dir.clone();
    let join_handle = run_runtime().spawn_blocking(move || {
        if execute_prepared_run_with_limits(prepared, event_callback).is_err() {
            let _ = unregister_run_handle(run_id);
            if let Ok(repo) = StorageRepository::open_in_app_data_dir(&app_data_dir) {
                let _ = repo.update_run_status(run_id, "failure", Some(&now_timestamp()));
            }
        }
    });
    set_run_task(run_id, join_handle)?;
    Ok(run_id)
}

fn prepare_run(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
    model: Option<&str>,
    effort_level: Option<&str>,
) -> RunManagerResult<PreparedRun> {
    let promptbook = load_promptbook(Path::new(promptbook_path))?;
    let workspace_path = Path::new(workspace_dir);
    let app_data_dir = workspace_path.join(".promptbook_runs");
    fs::create_dir_all(&app_data_dir)?;
    let repo = StorageRepository::open_in_app_data_dir(&app_data_dir)?;
    let max_parallel_runs = read_max_parallel_runs(&repo);

    let selected_agent = normalize_agent(agent).or_else(|| promptbook.defaults.agent.clone());
    let run_started_at = now_timestamp();
    let metadata = json!({
        "promptbook_path": promptbook_path,
        "workspace_dir": workspace_dir,
        "model": model,
        "effort_level": effort_level,
    });
    let metadata_json = Some(metadata.to_string());
    let run_id = repo.create_run(&NewRun {
        promptbook_name: promptbook.name.clone(),
        promptbook_version: promptbook.version.clone(),
        status: "running".to_string(),
        started_at: run_started_at,
        finished_at: None,
        agent_default: selected_agent.clone(),
        metadata_json,
    })?;

    // Pre-create all steps as "pending" so they are visible immediately
    for step in &promptbook.steps {
        repo.create_step(&NewStep {
            run_id,
            step_id: step.id.clone(),
            title: step.title.clone(),
            status: "pending".to_string(),
            started_at: None,
            finished_at: None,
            prompt: Some(step.prompt.clone()),
        })?;
    }

    Ok(PreparedRun {
        run_id,
        promptbook,
        selected_agent,
        selected_model: model.map(ToOwned::to_owned),
        selected_effort: effort_level.map(ToOwned::to_owned),
        workspace_path: workspace_path.to_path_buf(),
        app_data_dir,
        max_parallel_runs,
        from_step_id: None,
    })
}

fn execute_prepared_run_with_limits(
    prepared: PreparedRun,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<()> {
    let _parallel_run_slot = acquire_parallel_run_slot(prepared.max_parallel_runs)?;
    execute_prepared_run(prepared, event_callback)
}

fn execute_prepared_run(
    prepared: PreparedRun,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<()> {
    let run_id = prepared.run_id;
    let from_step_id = prepared.from_step_id.clone();
    let repo = match StorageRepository::open_in_app_data_dir(&prepared.app_data_dir) {
        Ok(repo) => repo,
        Err(err) => {
            let _ = unregister_run_handle(run_id);
            return Err(RunManagerError::from(err));
        }
    };
    let execution_result = execute_steps(
        &repo,
        run_id,
        &prepared.promptbook,
        prepared.selected_agent.as_deref(),
        prepared.selected_model.as_deref(),
        prepared.selected_effort.as_deref(),
        &prepared.workspace_path,
        &prepared.app_data_dir,
        from_step_id.as_deref(),
        event_callback.as_ref(),
    );

    let run_status = execution_result
        .as_ref()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|_| "failure".to_string());
    let run_finished_at = now_timestamp();
    let update_status_result = repo.update_run_status(run_id, &run_status, Some(&run_finished_at));
    emit_run_event(
        event_callback.as_ref(),
        RunEvent::RunFinished {
            run_id,
            status: run_status,
            ts: run_finished_at,
        },
    );
    let unregister_result = unregister_run_handle(run_id);

    if let Err(err) = update_status_result {
        return Err(RunManagerError::from(err));
    }
    if let Err(err) = unregister_result {
        return Err(err);
    }

    execution_result.map(|_| ())
}

pub fn cancel_run(run_id: i64) -> RunManagerResult<bool> {
    let process_handle = {
        let mut guard = run_handles()
            .lock()
            .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
        let Some(run_handle) = guard.get_mut(&run_id) else {
            return Ok(false);
        };
        run_handle.cancel_requested.store(true, Ordering::SeqCst);
        run_handle.current_process.clone()
    };

    if let Some(handle) = process_handle {
        let process = handle
            .lock()
            .map_err(|_| RunManagerError::ActiveRunState("process mutex poisoned".to_string()))?;
        process.cancel()?;
    }

    Ok(true)
}

fn execute_steps(
    repo: &StorageRepository,
    run_id: i64,
    promptbook: &PromptbookFile,
    selected_agent: Option<&str>,
    selected_model: Option<&str>,
    selected_effort: Option<&str>,
    workspace_path: &Path,
    app_data_dir: &Path,
    from_step_id: Option<&str>,
    event_callback: Option<&RunEventCallback>,
) -> RunManagerResult<String> {
    let default_agent = selected_agent
        .map(ToOwned::to_owned)
        .or_else(|| promptbook.defaults.agent.clone())
        .unwrap_or_else(|| "codex".to_string());
    let run_continue_on_error = promptbook
        .continue_on_error
        .or(promptbook.defaults.continue_on_error)
        .unwrap_or(false);
    let mut run_status = "success".to_string();

    let start_idx = match from_step_id {
        None => 0,
        Some(step_id) => promptbook
            .steps
            .iter()
            .position(|s| s.id == step_id)
            .unwrap_or(0),
    };

    for step in &promptbook.steps[start_idx..] {
        if is_run_cancelled(run_id)? {
            run_status = "failure".to_string();
            break;
        }

        let step_started_at = now_timestamp();
        // The step record was pre-created in prepare_run; just update its status
        repo.update_step_status(run_id, &step.id, "running", None)?;
        repo.update_step_started_at(run_id, &step.id, &step_started_at)?;
        emit_run_event(
            event_callback,
            RunEvent::StepStarted {
                run_id,
                step_id: step.id.clone(),
                title: step.title.clone(),
                ts: step_started_at,
            },
        );

        let step_prompt_path = write_step_task_file(app_data_dir, run_id, step, workspace_path)?;
        let step_agent = step.agent.as_deref().unwrap_or(default_agent.as_str());
        let adapter = create_adapter(step_agent)?;
        let adapter_options = AdapterOptions {
            model: selected_model.map(ToOwned::to_owned),
            effort_level: selected_effort.map(ToOwned::to_owned),
            ..AdapterOptions::default()
        };
        let command_spec = adapter.build_command(
            &step_prompt_path.to_string_lossy(),
            &workspace_path.to_string_lossy(),
            &adapter_options,
        );
        let args = command_spec.args.iter().map(String::as_str).collect::<Vec<_>>();
        let process_cwd = command_spec
            .cwd
            .clone()
            .unwrap_or_else(|| workspace_path.to_path_buf());
        let (process_handle, output_rx) = spawn_process(
            &command_spec.program,
            &args,
            ProcessOptions {
                cwd: Some(process_cwd),
                ..ProcessOptions::default()
            },
        )
        .map_err(|source| RunManagerError::ProcessSpawn {
            program: command_spec.program.clone(),
            source,
        })?;

        let process_handle = Arc::new(Mutex::new(process_handle));
        set_active_process(run_id, Some(Arc::clone(&process_handle)))?;

        let mut stdout_lines = Vec::new();
        let mut combined_lines = Vec::new();

        for event in output_rx {
            let stream = match event.stream {
                OutputStream::Stdout => "stdout",
                OutputStream::Stderr => "stderr",
            };
            let line = event.line;
            let ts = timestamp_for(event.ts);
            if event.stream == OutputStream::Stdout {
                stdout_lines.push(line.clone());
            }
            combined_lines.push(line.clone());

            repo.append_log_line(&NewLogLine {
                run_id,
                step_id: step.id.clone(),
                ts: ts.clone(),
                stream: stream.to_string(),
                line: line.clone(),
            })?;
            emit_run_event(
                event_callback,
                RunEvent::StepProgressLine {
                    run_id,
                    step_id: step.id.clone(),
                    stream: stream.to_string(),
                    line,
                    ts,
                },
            );
        }

        let process_exit = {
            let mut handle = process_handle
                .lock()
                .map_err(|_| RunManagerError::ActiveRunState("process mutex poisoned".to_string()))?;
            handle.wait().map_err(RunManagerError::ProcessWait)?
        };
        set_active_process(run_id, None)?;

        let output_content = if !stdout_lines.is_empty() {
            stdout_lines.join("\n")
        } else {
            combined_lines.join("\n")
        };
        repo.set_step_output(&StepOutput {
            run_id,
            step_id: step.id.clone(),
            ts: now_timestamp(),
            content: output_content,
            format: "text/plain".to_string(),
        })?;

        let step_failed = !process_exit.success
            || process_exit.cancelled
            || process_exit.timed_out
            || is_run_cancelled(run_id)?;
        repo.update_step_status(
            run_id,
            &step.id,
            if step_failed { "failure" } else { "success" },
            Some(&now_timestamp()),
        )?;
        emit_run_event(
            event_callback,
            RunEvent::StepFinished {
                run_id,
                step_id: step.id.clone(),
                status: if step_failed {
                    "failure".to_string()
                } else {
                    "success".to_string()
                },
                ts: now_timestamp(),
            },
        );

        if step_failed {
            run_status = "failure".to_string();
            let continue_on_error = step.continue_on_error.unwrap_or(run_continue_on_error);
            if !continue_on_error {
                break;
            }
        }
    }

    Ok(run_status)
}

fn emit_run_event(event_callback: Option<&RunEventCallback>, event: RunEvent) {
    if let Some(callback) = event_callback {
        callback(event);
    }
}

fn create_adapter(agent_name: &str) -> RunManagerResult<Box<dyn AgentAdapter>> {
    match agent_name {
        "codex" => Ok(Box::new(CodexAdapter)),
        "claude" => Ok(Box::new(ClaudeAdapter)),
        "copilot" => Ok(Box::new(CopilotAdapter)),
        "dry-run" => Ok(Box::new(DryRunAdapter)),
        _ => Err(RunManagerError::UnknownAgent(agent_name.to_string())),
    }
}

fn load_promptbook(path: &Path) -> RunManagerResult<PromptbookFile> {
    let raw = fs::read_to_string(path)?;
    let promptbook: PromptbookFile = serde_yaml::from_str(&raw)
        .map_err(|err| RunManagerError::PromptbookParse(err.to_string()))?;

    if promptbook.schema_version != "promptbook/v1" {
        return Err(RunManagerError::PromptbookParse(format!(
            "unsupported schema_version `{}`",
            promptbook.schema_version
        )));
    }

    if promptbook.steps.is_empty() {
        return Err(RunManagerError::PromptbookParse(
            "promptbook requires at least one step".to_string(),
        ));
    }

    for step in &promptbook.steps {
        if step.id.trim().is_empty() {
            return Err(RunManagerError::PromptbookParse(
                "step.id cannot be empty".to_string(),
            ));
        }
        if step.title.trim().is_empty() {
            return Err(RunManagerError::PromptbookParse(
                "step.title cannot be empty".to_string(),
            ));
        }
    }

    Ok(promptbook)
}

fn write_step_task_file(
    app_data_dir: &Path,
    run_id: i64,
    step: &PromptbookStep,
    workspace_path: &Path,
) -> RunManagerResult<PathBuf> {
    let steps_dir = app_data_dir.join(run_id.to_string()).join("steps");
    fs::create_dir_all(&steps_dir)?;

    let file_name = format!("{}.md", sanitize_path_segment(&step.id));
    let file_path = steps_dir.join(file_name);
    let verify_lines = if step.verify.is_empty() {
        "- none".to_string()
    } else {
        step.verify
            .iter()
            .map(|command| format!("- `{command}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let file_contents = format!(
        "# Step: {}\n\nStep ID: `{}`\nWorkspace: `{}`\n\n## Task\n{}\n\n## Verify\n{}\n",
        step.title,
        step.id,
        workspace_path.display(),
        step.prompt.trim(),
        verify_lines
    );
    fs::write(&file_path, file_contents)?;

    Ok(file_path)
}

fn now_timestamp() -> String {
    timestamp_for(SystemTime::now())
}

fn timestamp_for(ts: SystemTime) -> String {
    let duration = ts.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}Z", duration.as_secs(), duration.subsec_millis())
}

fn sanitize_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }

    if output.is_empty() {
        "step".to_string()
    } else {
        output
    }
}

fn normalize_agent(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(ToOwned::to_owned)
}

fn run_runtime() -> &'static TokioRuntime {
    RUN_RUNTIME.get_or_init(|| {
        TokioRuntimeBuilder::new_multi_thread()
            .enable_all()
            .thread_name("promptbook-runner")
            .build()
            .expect("failed to create run manager tokio runtime")
    })
}

fn run_handles() -> &'static Mutex<HashMap<i64, RunHandle>> {
    RUN_HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn parallel_run_limiter() -> &'static ParallelRunLimiter {
    PARALLEL_RUN_LIMITER.get_or_init(ParallelRunLimiter::new)
}

fn read_max_parallel_runs(repo: &StorageRepository) -> usize {
    if let Ok(value) = std::env::var("PROMPTBOOK_MAX_PARALLEL_RUNS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed > 0 {
                return parsed;
            }
        }
    }

    if let Ok(Some(value_json)) = repo.get_setting_value_json(MAX_PARALLEL_RUNS_SETTING_KEY) {
        if let Some(parsed) = parse_max_parallel_runs_setting(&value_json) {
            return parsed;
        }
    }

    DEFAULT_MAX_PARALLEL_RUNS
}

fn parse_max_parallel_runs_setting(raw_value: &str) -> Option<usize> {
    if let Ok(parsed) = serde_json::from_str::<usize>(raw_value) {
        return (parsed > 0).then_some(parsed);
    }
    if let Ok(parsed) = raw_value.trim().parse::<usize>() {
        return (parsed > 0).then_some(parsed);
    }
    if let Ok(as_string) = serde_json::from_str::<String>(raw_value) {
        if let Ok(parsed) = as_string.trim().parse::<usize>() {
            return (parsed > 0).then_some(parsed);
        }
    }
    None
}

struct ParallelRunSlot;

impl Drop for ParallelRunSlot {
    fn drop(&mut self) {
        let limiter = parallel_run_limiter();
        if let Ok(mut state) = limiter.state.lock() {
            if state.active_runs > 0 {
                state.active_runs -= 1;
            }
            limiter.condvar.notify_one();
        }
    }
}

fn acquire_parallel_run_slot(max_parallel_runs: usize) -> RunManagerResult<ParallelRunSlot> {
    let resolved_max = max_parallel_runs.max(1);
    let limiter = parallel_run_limiter();
    let mut state = limiter.state.lock().map_err(|_| {
        RunManagerError::ActiveRunState("parallel run limiter mutex poisoned".to_string())
    })?;
    if state.max_parallel_runs != resolved_max {
        state.max_parallel_runs = resolved_max;
        limiter.condvar.notify_all();
    }
    while state.active_runs >= state.max_parallel_runs {
        state = limiter.condvar.wait(state).map_err(|_| {
            RunManagerError::ActiveRunState("parallel run limiter mutex poisoned".to_string())
        })?;
    }
    state.active_runs += 1;
    Ok(ParallelRunSlot)
}

fn register_run_handle(run_id: i64) -> RunManagerResult<()> {
    let mut guard = run_handles()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    guard.insert(
        run_id,
        RunHandle {
            cancel_requested: Arc::new(AtomicBool::new(false)),
            current_process: None,
            task: None,
        },
    );
    Ok(())
}

fn set_run_task(run_id: i64, task: TokioJoinHandle<()>) -> RunManagerResult<()> {
    let mut guard = run_handles()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    if let Some(run_handle) = guard.get_mut(&run_id) {
        run_handle.task = Some(task);
    }
    Ok(())
}

fn unregister_run_handle(run_id: i64) -> RunManagerResult<()> {
    let mut guard = run_handles()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    guard.remove(&run_id);
    Ok(())
}

fn set_active_process(
    run_id: i64,
    process: Option<Arc<Mutex<ProcessHandle>>>,
) -> RunManagerResult<()> {
    let mut guard = run_handles()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    let Some(run_handle) = guard.get_mut(&run_id) else {
        return Err(RunManagerError::ActiveRunState(format!(
            "run_id {run_id} is not active"
        )));
    };
    run_handle.current_process = process;
    Ok(())
}

fn is_run_cancelled(run_id: i64) -> RunManagerResult<bool> {
    let guard = run_handles()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    Ok(guard
        .get(&run_id)
        .map(|run_handle| run_handle.cancel_requested.load(Ordering::SeqCst))
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::StorageRepository;

    use super::{cancel_run, run_promptbook, start_run_background, start_run_background_from, RunManagerError};

    fn run_manager_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("run manager test lock")
    }

    fn temp_workspace_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time moved backwards")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "promptbook-runner-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create workspace dir");
        directory
    }

    fn write_two_step_fixture(path: &Path) -> PathBuf {
        let promptbook_path = path.join("two-step-dry-run.v1.yaml");
        let promptbook_yaml = r#"
schema_version: "promptbook/v1"
name: "dry-run-two-step"
version: "1.0.0"
description: "Fixture for run manager test"
steps:
  - id: "step-1"
    title: "First step"
    prompt: |
      Run first step
    verify:
      - "echo ok"
  - id: "step-2"
    title: "Second step"
    prompt: |
      Run second step
    verify:
      - "echo ok"
"#;
        fs::write(&promptbook_path, promptbook_yaml.trim_start())
            .expect("write promptbook fixture");
        promptbook_path
    }

    fn write_large_fixture(path: &Path, step_count: usize) -> PathBuf {
        let promptbook_path = path.join("large-dry-run.v1.yaml");
        let mut promptbook_yaml = String::from(
            "schema_version: \"promptbook/v1\"\nname: \"dry-run-large\"\nversion: \"1.0.0\"\ndescription: \"Fixture for cancellation\"\nsteps:\n",
        );
        for step in 0..step_count {
            let step_number = step + 1;
            promptbook_yaml.push_str(&format!(
                "  - id: \"step-{step_number}\"\n    title: \"Step {step_number}\"\n    prompt: \"Run step {step_number}\"\n    verify:\n      - \"echo ok\"\n"
            ));
        }
        fs::write(&promptbook_path, promptbook_yaml).expect("write large promptbook fixture");
        promptbook_path
    }

    fn write_promptbook(path: &Path, file_name: &str, contents: &str) -> PathBuf {
        let promptbook_path = path.join(file_name);
        fs::write(&promptbook_path, contents).expect("write promptbook");
        promptbook_path
    }

    fn wait_for_run_completion(app_data_dir: &Path, run_id: i64) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let repo = StorageRepository::open_in_app_data_dir(app_data_dir).expect("open db");
            let detail = repo
                .get_run_detail(run_id)
                .expect("get run detail")
                .expect("run detail exists");
            if detail.run.status != "running" {
                return;
            }
            if std::time::Instant::now() >= deadline {
                panic!("run {run_id} did not finish before timeout");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn dry_run_two_step_promptbook_persists_steps_logs_and_outputs() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-dry-run");
        let promptbook_path = write_two_step_fixture(&workspace_dir);

        let run_id = run_promptbook(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
        )
        .expect("run promptbook");

        let app_data_dir = workspace_dir.join(".promptbook_runs");
        let repo = StorageRepository::open_in_app_data_dir(&app_data_dir).expect("open db");
        let detail = repo
            .get_run_detail(run_id)
            .expect("get run detail")
            .expect("run detail exists");

        assert_eq!(detail.run.status, "success");
        assert_eq!(detail.steps.len(), 2);
        assert!(detail.steps.iter().all(|step| step.status == "success"));
        assert!(detail.steps.iter().all(|s| s.prompt.is_some()), "all steps should have prompt");
        assert!(!detail.logs.is_empty(), "expected persisted logs");
        assert_eq!(detail.outputs.len(), 2);
        assert!(
            detail
                .outputs
                .iter()
                .all(|output| output.content.contains("FINAL: ok")),
            "expected FINAL output for each step"
        );

        let step_one_prompt_path = app_data_dir
            .join(run_id.to_string())
            .join("steps")
            .join("step-1.md");
        let step_two_prompt_path = app_data_dir
            .join(run_id.to_string())
            .join("steps")
            .join("step-2.md");
        assert!(step_one_prompt_path.exists(), "missing step-1 prompt file");
        assert!(step_two_prompt_path.exists(), "missing step-2 prompt file");

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn starts_two_dry_runs_in_parallel_and_persists_each_run() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-dry-run-parallel");
        let promptbook_path = write_two_step_fixture(&workspace_dir);

        let run_id_one = start_run_background(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("start first run");
        let run_id_two = start_run_background(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("start second run");
        assert_ne!(run_id_one, run_id_two);

        let app_data_dir = workspace_dir.join(".promptbook_runs");
        wait_for_run_completion(&app_data_dir, run_id_one);
        wait_for_run_completion(&app_data_dir, run_id_two);

        let repo = StorageRepository::open_in_app_data_dir(&app_data_dir).expect("open db");
        for run_id in [run_id_one, run_id_two] {
            let detail = repo
                .get_run_detail(run_id)
                .expect("get run detail")
                .expect("run detail exists");
            assert_eq!(detail.run.status, "success");
            assert_eq!(detail.steps.len(), 2);
            assert!(detail.steps.iter().all(|step| step.status == "success"));
            assert!(!detail.logs.is_empty(), "expected persisted logs");
            assert_eq!(detail.outputs.len(), 2);
            assert!(
                detail
                    .outputs
                    .iter()
                    .all(|output| output.content.contains("FINAL: ok")),
                "expected FINAL output for each step"
            );
        }

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn rejects_empty_promptbook_steps() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-empty-steps");
        let promptbook_path = write_promptbook(
            &workspace_dir,
            "empty-steps.v1.yaml",
            r#"
schema_version: "promptbook/v1"
name: "empty"
version: "1.0.0"
description: "No steps"
steps: []
"#
            .trim_start(),
        );

        let result = run_promptbook(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
        );

        match result {
            Err(RunManagerError::PromptbookParse(message)) => {
                assert!(message.contains("at least one step"));
            }
            _ => panic!("expected promptbook parse error for empty steps"),
        }

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn rejects_invalid_yaml_promptbook() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-invalid-yaml");
        let promptbook_path = write_promptbook(
            &workspace_dir,
            "invalid-yaml.v1.yaml",
            "schema_version: \"promptbook/v1\"\nname: \"bad\"\nsteps: [\n",
        );

        let result = run_promptbook(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
        );

        assert!(
            matches!(result, Err(RunManagerError::PromptbookParse(_))),
            "expected invalid yaml parse error, got: {result:?}"
        );

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn rejects_unknown_adapter() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-unknown-adapter");
        let promptbook_path = write_two_step_fixture(&workspace_dir);

        let result = run_promptbook(
            &promptbook_path.to_string_lossy(),
            Some("unknown-adapter"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
        );

        match result {
            Err(RunManagerError::UnknownAgent(agent_name)) => {
                assert_eq!(agent_name, "unknown-adapter");
            }
            _ => panic!("expected unknown adapter error"),
        }

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn cancels_background_run_and_marks_it_failure() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-cancel");
        let promptbook_path = write_large_fixture(&workspace_dir, 120);

        let run_id = start_run_background(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
            None,
        )
        .expect("start run");

        let cancel_result = cancel_run(run_id).expect("cancel run");
        assert!(cancel_result, "run should be cancellable while active");

        let app_data_dir = workspace_dir.join(".promptbook_runs");
        wait_for_run_completion(&app_data_dir, run_id);

        let repo = StorageRepository::open_in_app_data_dir(&app_data_dir).expect("open db");
        let detail = repo
            .get_run_detail(run_id)
            .expect("get run detail")
            .expect("run detail exists");

        assert_eq!(detail.run.status, "failure");
        let executed_steps = detail.steps.iter().filter(|s| s.status != "pending").count();
        assert!(
            executed_steps < 120,
            "expected cancellation before all steps execute, got {} executed steps",
            executed_steps
        );

        let _ = fs::remove_dir_all(workspace_dir);
    }

    fn write_three_step_fixture(path: &Path) -> PathBuf {
        let promptbook_path = path.join("three-step-dry-run.v1.yaml");
        let promptbook_yaml = r#"
schema_version: "promptbook/v1"
name: "dry-run-three-step"
version: "1.0.0"
description: "Fixture for resume test"
steps:
  - id: "step-1"
    title: "First step"
    prompt: |
      Run first step
    verify:
      - "echo ok"
  - id: "step-2"
    title: "Second step"
    prompt: |
      Run second step
    verify:
      - "echo ok"
  - id: "step-3"
    title: "Third step"
    prompt: |
      Run third step
    verify:
      - "echo ok"
"#;
        fs::write(&promptbook_path, promptbook_yaml.trim_start())
            .expect("write promptbook fixture");
        promptbook_path
    }

    #[test]
    fn resume_run_from_step_skips_earlier_steps() {
        let _guard = run_manager_test_lock();
        let workspace_dir = temp_workspace_dir("run-manager-resume");
        let promptbook_path = write_three_step_fixture(&workspace_dir);

        let run_id = start_run_background_from(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
            None,
            None,
            Some("step-2"),
            None,
        )
        .expect("start run from step-2");

        let app_data_dir = workspace_dir.join(".promptbook_runs");
        wait_for_run_completion(&app_data_dir, run_id);

        let repo = StorageRepository::open_in_app_data_dir(&app_data_dir).expect("open db");
        let detail = repo
            .get_run_detail(run_id)
            .expect("get run detail")
            .expect("run detail exists");

        assert_eq!(detail.run.status, "success");
        assert_eq!(detail.steps.len(), 3, "all three steps pre-created");
        assert_eq!(detail.steps[0].step_id, "step-1");
        assert_eq!(detail.steps[0].status, "pending", "step-1 was skipped so stays pending");
        assert_eq!(detail.steps[1].step_id, "step-2");
        assert_eq!(detail.steps[1].status, "success");
        assert_eq!(detail.steps[2].step_id, "step-3");
        assert_eq!(detail.steps[2].status, "success");

        let _ = fs::remove_dir_all(workspace_dir);
    }
}
