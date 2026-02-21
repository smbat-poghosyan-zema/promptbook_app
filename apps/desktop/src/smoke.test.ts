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
        metadata_json: null
      },
      {
        id: 102,
        promptbook_name: "hotfix",
        promptbook_version: "1.1.0",
        status: "success",
        started_at: "2026-02-21T00:05:00Z",
        finished_at: "2026-02-21T00:06:00Z",
        agent_default: "claude",
        metadata_json: null
      }
    ];

    const viewModel = createRunListViewModel(runs);

    expect(viewModel.length).toBe(2);
    expect(viewModel.some((item) => item.subtitle.includes("running"))).toBe(true);
    expect(viewModel.some((item) => item.subtitle.includes("2026-02-21T00:05:00Z"))).toBe(true);
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
        metadata_json: null
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
    expect(viewModel.liveLines.some((line) => line.includes("planning..."))).toBe(true);
    expect(viewModel.selectedOutput?.content).toBe("Final answer A");
  });
});
