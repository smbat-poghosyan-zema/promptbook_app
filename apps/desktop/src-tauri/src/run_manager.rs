use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::agent_adapter::{
    AdapterOptions, AgentAdapter, ClaudeAdapter, CodexAdapter, CopilotAdapter, DryRunAdapter,
};
use crate::process_exec::{spawn_process, OutputStream, ProcessHandle, ProcessOptions};
use crate::{NewLogLine, NewRun, NewStep, StepOutput, StorageError, StorageRepository};

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
struct ActiveRunState {
    cancel_requested: Arc<AtomicBool>,
    current_process: Option<Arc<Mutex<ProcessHandle>>>,
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
    workspace_path: PathBuf,
    app_data_dir: PathBuf,
}

static ACTIVE_RUNS: OnceLock<Mutex<HashMap<i64, ActiveRunState>>> = OnceLock::new();

pub fn run_promptbook(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
) -> RunManagerResult<i64> {
    let prepared = prepare_run(promptbook_path, agent, workspace_dir)?;
    let run_id = prepared.run_id;
    register_active_run(run_id)?;
    execute_prepared_run(prepared, None)?;
    Ok(run_id)
}

pub fn start_run_background(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<i64> {
    let prepared = prepare_run(promptbook_path, agent, workspace_dir)?;
    let run_id = prepared.run_id;
    register_active_run(run_id)?;

    let app_data_dir = prepared.app_data_dir.clone();
    let spawn_result = thread::Builder::new()
        .name(format!("promptbook-run-{run_id}"))
        .spawn(move || {
            let _ = execute_prepared_run(prepared, event_callback);
        });

    if let Err(err) = spawn_result {
        let _ = unregister_active_run(run_id);
        if let Ok(repo) = StorageRepository::open_in_app_data_dir(&app_data_dir) {
            let _ = repo.update_run_status(run_id, "failure", Some(&now_timestamp()));
        }
        return Err(RunManagerError::ActiveRunState(format!(
            "failed to spawn run thread: {err}"
        )));
    }

    Ok(run_id)
}

fn prepare_run(
    promptbook_path: &str,
    agent: Option<&str>,
    workspace_dir: &str,
) -> RunManagerResult<PreparedRun> {
    let promptbook = load_promptbook(Path::new(promptbook_path))?;
    let workspace_path = Path::new(workspace_dir);
    let app_data_dir = workspace_path.join(".promptbook_runs");
    fs::create_dir_all(&app_data_dir)?;
    let repo = StorageRepository::open_in_app_data_dir(&app_data_dir)?;

    let selected_agent = normalize_agent(agent).or_else(|| promptbook.defaults.agent.clone());
    let run_started_at = now_timestamp();
    let run_id = repo.create_run(&NewRun {
        promptbook_name: promptbook.name.clone(),
        promptbook_version: promptbook.version.clone(),
        status: "running".to_string(),
        started_at: run_started_at,
        finished_at: None,
        agent_default: selected_agent.clone(),
        metadata_json: Some(format!("{{\"promptbook_path\":\"{promptbook_path}\"}}")),
    })?;

    Ok(PreparedRun {
        run_id,
        promptbook,
        selected_agent,
        workspace_path: workspace_path.to_path_buf(),
        app_data_dir,
    })
}

fn execute_prepared_run(
    prepared: PreparedRun,
    event_callback: Option<RunEventCallback>,
) -> RunManagerResult<()> {
    let run_id = prepared.run_id;
    let repo = match StorageRepository::open_in_app_data_dir(&prepared.app_data_dir) {
        Ok(repo) => repo,
        Err(err) => {
            let _ = unregister_active_run(run_id);
            return Err(RunManagerError::from(err));
        }
    };
    let execution_result = execute_steps(
        &repo,
        run_id,
        &prepared.promptbook,
        prepared.selected_agent.as_deref(),
        &prepared.workspace_path,
        &prepared.app_data_dir,
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
    let unregister_result = unregister_active_run(run_id);

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
        let mut guard = active_runs()
            .lock()
            .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
        let Some(active_run) = guard.get_mut(&run_id) else {
            return Ok(false);
        };
        active_run.cancel_requested.store(true, Ordering::SeqCst);
        active_run.current_process.clone()
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
    workspace_path: &Path,
    app_data_dir: &Path,
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

    for step in &promptbook.steps {
        if is_run_cancelled(run_id)? {
            run_status = "failure".to_string();
            break;
        }

        let step_started_at = now_timestamp();
        repo.create_step(&NewStep {
            run_id,
            step_id: step.id.clone(),
            title: step.title.clone(),
            status: "running".to_string(),
            started_at: Some(step_started_at.clone()),
            finished_at: None,
        })?;
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
        let adapter_options = AdapterOptions::default();
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

fn active_runs() -> &'static Mutex<HashMap<i64, ActiveRunState>> {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_active_run(run_id: i64) -> RunManagerResult<()> {
    let mut guard = active_runs()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    guard.insert(
        run_id,
        ActiveRunState {
            cancel_requested: Arc::new(AtomicBool::new(false)),
            current_process: None,
        },
    );
    Ok(())
}

fn unregister_active_run(run_id: i64) -> RunManagerResult<()> {
    let mut guard = active_runs()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    guard.remove(&run_id);
    Ok(())
}

fn set_active_process(
    run_id: i64,
    process: Option<Arc<Mutex<ProcessHandle>>>,
) -> RunManagerResult<()> {
    let mut guard = active_runs()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    let Some(active_run) = guard.get_mut(&run_id) else {
        return Err(RunManagerError::ActiveRunState(format!(
            "run_id {run_id} is not active"
        )));
    };
    active_run.current_process = process;
    Ok(())
}

fn is_run_cancelled(run_id: i64) -> RunManagerResult<bool> {
    let guard = active_runs()
        .lock()
        .map_err(|_| RunManagerError::ActiveRunState("active run mutex poisoned".to_string()))?;
    Ok(guard
        .get(&run_id)
        .map(|active| active.cancel_requested.load(Ordering::SeqCst))
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::StorageRepository;

    use super::run_promptbook;

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

    #[test]
    fn dry_run_two_step_promptbook_persists_steps_logs_and_outputs() {
        let workspace_dir = temp_workspace_dir("run-manager-dry-run");
        let promptbook_path = write_two_step_fixture(&workspace_dir);

        let run_id = run_promptbook(
            &promptbook_path.to_string_lossy(),
            Some("dry-run"),
            &workspace_dir.to_string_lossy(),
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
}
