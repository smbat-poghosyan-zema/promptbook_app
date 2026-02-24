import { ipcRunPromptbookRequestSchema } from "@promptbook/shared";
import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  type IpcRunDetail,
  type IpcRunRecord
} from "./ui-model.ts";

describe("shared schema smoke", () => {
  it("parses a minimal run payload", () => {
    const payload = ipcRunPromptbookRequestSchema.parse({
      promptbookPath: "/tmp/hello.v1.yaml"
    });

    expect(payload.promptbookPath).toBe("/tmp/hello.v1.yaml");
  });

  it("Run list renders mock data", () => {
    const runs: IpcRunRecord[] = [
      {
        id: 101,
        promptbook_name: "release",
        promptbook_version: "1.0.0",
        status: "running",
        started_at: "2026-02-21T00:00:00Z",
        finished_at: null,
        agent_default: "codex",
        metadata_json: null,
        model: null,
        effort_level: null,
        workspace_dir: null,
        step_count: 0,
        current_step_title: null
      },
      {
        id: 102,
        promptbook_name: "hotfix",
        promptbook_version: "1.1.0",
        status: "success",
        started_at: "2026-02-21T00:05:00Z",
        finished_at: "2026-02-21T00:06:00Z",
        agent_default: "claude",
        metadata_json: null,
        model: null,
        effort_level: null,
        workspace_dir: "/home/user/project",
        step_count: 2,
        current_step_title: null
      }
    ];

    const viewModel = createRunListViewModel(runs);

    expect(viewModel.length).toBe(2);
    expect(viewModel.some((item) => item.status === "running")).toBe(true);
    expect(viewModel.some((item) => item.workspaceLine === "/home/user/project")).toBe(true);
  });

  it("Run detail shows split views with mock events", () => {
    const detail: IpcRunDetail = {
      run: {
        id: 101,
        promptbook_name: "release",
        promptbook_version: "1.0.0",
        status: "running",
        started_at: "2026-02-21T00:00:00Z",
        finished_at: null,
        agent_default: "codex",
        metadata_json: null,
        model: null,
        effort_level: null,
        workspace_dir: null,
        step_count: 0,
        current_step_title: null
      },
      steps: [
        {
          id: 1,
          run_id: 101,
          step_id: "step-a",
          title: "Create summary",
          status: "running",
          started_at: "2026-02-21T00:00:01Z",
          finished_at: null
        }
      ],
      logs: [],
      outputs: [
        {
          id: 10,
          run_id: 101,
          step_id: "step-a",
          ts: "2026-02-21T00:00:03Z",
          content: "Final answer A",
          format: "markdown"
        }
      ]
    };

    const updated = applyRunEvent(detail, {
      run_id: 101,
      step_id: "step-a",
      type: "step_progress_line",
      payload: {
        stream: "stdout",
        line: "planning...",
        ts: "2026-02-21T00:00:02Z"
      }
    });

    const viewModel = createRunDetailViewModel(updated, "step-a");

    expect(viewModel.liveProgressTitle).toBe("Live Progress");
    expect(viewModel.finalOutputTitle).toBe("Final Output");
    expect(viewModel.activeStep?.liveLines.some((line) => line.includes("planning..."))).toBe(true);
    expect(viewModel.activeStep?.finalOutput).toBe("Final answer A");
  });
});
