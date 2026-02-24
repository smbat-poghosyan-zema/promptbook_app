use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingBehavior {
    Streaming,
    MostlyFinal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: Vec<String>, cwd: Option<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterOptions {
    pub system_prompt: String,
    pub prompt_override: Option<String>,
    pub allow_all_tools: bool,
    pub model: Option<String>,
    pub effort_level: Option<String>,
}

impl Default for AdapterOptions {
    fn default() -> Self {
        Self {
            system_prompt: "You are a coding agent executing promptbook steps.".to_string(),
            prompt_override: None,
            allow_all_tools: false,
            model: None,
            effort_level: None,
        }
    }
}

pub trait AgentAdapter {
    fn name(&self) -> &'static str;
    fn build_command(
        &self,
        step_prompt_file_path: &str,
        workspace_dir: &str,
        options: &AdapterOptions,
    ) -> CommandSpec;
    fn expected_streaming_behavior(&self) -> StreamingBehavior;
}

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn build_command(
        &self,
        step_prompt_file_path: &str,
        workspace_dir: &str,
        options: &AdapterOptions,
    ) -> CommandSpec {
        let instruction = options.prompt_override.clone().unwrap_or_else(|| {
            format!(
                "{}\n\nUse step prompt file: {}\nWorkspace: {}",
                options.system_prompt, step_prompt_file_path, workspace_dir
            )
        });

        let mut args = vec![
            "exec".to_string(),
            "--full-auto".to_string(),
            "--sandbox".to_string(),
            "workspace-write".to_string(),
        ];
        if let Some(ref model) = options.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        args.push(instruction);

        CommandSpec::new("codex", args, Some(PathBuf::from(workspace_dir)))
    }

    fn expected_streaming_behavior(&self) -> StreamingBehavior {
        // Codex progress updates are typically streamed on stderr.
        StreamingBehavior::Streaming
    }
}

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn build_command(
        &self,
        step_prompt_file_path: &str,
        workspace_dir: &str,
        options: &AdapterOptions,
    ) -> CommandSpec {
        let mut cmd_parts = vec![format!("cat {}", shell_single_quote(step_prompt_file_path))];
        cmd_parts.push("|".to_string());
        cmd_parts.push("claude".to_string());
        if let Some(ref model) = options.model {
            cmd_parts.push(format!("--model {}", shell_single_quote(model)));
        }
        if let Some(ref effort) = options.effort_level {
            cmd_parts.push(format!("--effort {}", shell_single_quote(effort)));
        }
        cmd_parts.push("-p".to_string());
        cmd_parts.push(shell_single_quote(&options.system_prompt));
        let command = cmd_parts.join(" ");

        CommandSpec::new(
            "bash",
            vec!["-lc".to_string(), command],
            Some(PathBuf::from(workspace_dir)),
        )
    }

    fn expected_streaming_behavior(&self) -> StreamingBehavior {
        StreamingBehavior::MostlyFinal
    }
}

pub struct CopilotAdapter;

impl AgentAdapter for CopilotAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn build_command(
        &self,
        step_prompt_file_path: &str,
        workspace_dir: &str,
        options: &AdapterOptions,
    ) -> CommandSpec {
        let prompt = options.prompt_override.clone().unwrap_or_else(|| {
            format!(
                "{}\n\nRead and follow instructions in: {}",
                options.system_prompt, step_prompt_file_path
            )
        });

        let mut args = vec!["-p".to_string(), prompt];
        if options.allow_all_tools {
            args.push("--allow-all-tools".to_string());
        }

        CommandSpec::new("copilot", args, Some(PathBuf::from(workspace_dir)))
    }

    fn expected_streaming_behavior(&self) -> StreamingBehavior {
        StreamingBehavior::MostlyFinal
    }
}

pub struct DryRunAdapter;

impl AgentAdapter for DryRunAdapter {
    fn name(&self) -> &'static str {
        "dry-run"
    }

    fn build_command(
        &self,
        _step_prompt_file_path: &str,
        workspace_dir: &str,
        _options: &AdapterOptions,
    ) -> CommandSpec {
        CommandSpec::new(
            "bash",
            vec![
                "-lc".to_string(),
                "echo \"FINAL: ok\"; echo \"progress...\" 1>&2".to_string(),
            ],
            Some(PathBuf::from(workspace_dir)),
        )
    }

    fn expected_streaming_behavior(&self) -> StreamingBehavior {
        StreamingBehavior::Streaming
    }
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub supports_effort: bool,
}

pub fn models_for_agent(agent_name: &str) -> Vec<ModelInfo> {
    match agent_name {
        "claude" => vec![
            ModelInfo { id: "claude-opus-4-5".into(), name: "Claude Opus 4.5".into(), supports_effort: true },
            ModelInfo { id: "claude-sonnet-4-5".into(), name: "Claude Sonnet 4.5".into(), supports_effort: true },
            ModelInfo { id: "claude-sonnet-4-6".into(), name: "Claude Sonnet 4.6".into(), supports_effort: true },
            ModelInfo { id: "claude-haiku-3-5".into(), name: "Claude Haiku 3.5".into(), supports_effort: false },
        ],
        "codex" => vec![
            ModelInfo { id: "o4-mini".into(), name: "o4-mini".into(), supports_effort: true },
            ModelInfo { id: "o3".into(), name: "o3".into(), supports_effort: true },
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterOptions, AgentAdapter, ClaudeAdapter, CodexAdapter, CopilotAdapter, DryRunAdapter,
        StreamingBehavior,
    };
    use crate::process_exec::{spawn_process, OutputStream, ProcessOptions};

    fn default_options() -> AdapterOptions {
        AdapterOptions {
            system_prompt: "You are a helpful coding agent".to_string(),
            prompt_override: None,
            allow_all_tools: false,
            model: None,
            effort_level: None,
        }
    }

    #[test]
    fn codex_adapter_builds_non_empty_program_and_args() {
        let adapter = CodexAdapter;
        let spec = adapter.build_command("/tmp/step.md", "/tmp/workspace", &default_options());

        assert!(!spec.program.trim().is_empty());
        assert!(!spec.args.is_empty());
        assert_eq!(adapter.name(), "codex");
        assert_eq!(adapter.expected_streaming_behavior(), StreamingBehavior::Streaming);
    }

    #[test]
    fn claude_adapter_builds_non_empty_program_and_args() {
        let adapter = ClaudeAdapter;
        let spec = adapter.build_command("/tmp/step.md", "/tmp/workspace", &default_options());

        assert!(!spec.program.trim().is_empty());
        assert!(!spec.args.is_empty());
        assert_eq!(adapter.name(), "claude");
        assert_eq!(
            adapter.expected_streaming_behavior(),
            StreamingBehavior::MostlyFinal
        );
    }

    #[test]
    fn copilot_adapter_builds_non_empty_program_and_args() {
        let adapter = CopilotAdapter;
        let spec = adapter.build_command("/tmp/step.md", "/tmp/workspace", &default_options());

        assert!(!spec.program.trim().is_empty());
        assert!(!spec.args.is_empty());
        assert_eq!(adapter.name(), "copilot");
        assert_eq!(adapter.expected_streaming_behavior(), StreamingBehavior::MostlyFinal);
    }

    #[test]
    fn dry_run_adapter_builds_non_empty_program_and_args() {
        let adapter = DryRunAdapter;
        let spec = adapter.build_command("/tmp/step.md", "/tmp/workspace", &default_options());

        assert!(!spec.program.trim().is_empty());
        assert!(!spec.args.is_empty());
        assert_eq!(adapter.name(), "dry-run");
        assert_eq!(adapter.expected_streaming_behavior(), StreamingBehavior::Streaming);
    }

    #[test]
    fn dry_run_adapter_emits_progress_and_final_output() {
        let adapter = DryRunAdapter;
        let spec = adapter.build_command("/tmp/step.md", "/tmp/workspace", &default_options());
        let arg_refs = spec.args.iter().map(String::as_str).collect::<Vec<_>>();

        let (mut handle, output_rx) = spawn_process(&spec.program, &arg_refs, ProcessOptions::default())
            .expect("spawn dry run command");

        let exit = handle.wait().expect("wait for dry run process");
        assert!(exit.success, "dry run process should succeed: {exit:?}");

        let events = output_rx.into_iter().collect::<Vec<_>>();
        let stderr_lines = events
            .iter()
            .filter(|event| event.stream == OutputStream::Stderr)
            .map(|event| event.line.as_str())
            .collect::<Vec<_>>();

        let stdout_lines = events
            .iter()
            .filter(|event| event.stream == OutputStream::Stdout)
            .map(|event| event.line.as_str())
            .collect::<Vec<_>>();

        assert!(
            stderr_lines.iter().any(|line| line.contains("progress...")),
            "expected at least one stderr progress line, got: {stderr_lines:?}"
        );
        assert_eq!(
            stdout_lines,
            vec!["FINAL: ok"],
            "expected one stdout final line"
        );
    }

    #[test]
    fn models_for_agent_returns_claude_models_with_effort_support() {
        use super::models_for_agent;
        let models = models_for_agent("claude");
        assert!(models.len() >= 2, "expected at least 2 claude models");
        let supports_effort_count = models.iter().filter(|m| m.supports_effort).count();
        assert!(
            supports_effort_count >= 2,
            "expected at least 2 claude models with supports_effort=true, got: {supports_effort_count}"
        );
    }
}
