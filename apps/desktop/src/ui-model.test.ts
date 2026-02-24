import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  type IpcRunDetail,
  type IpcRunRecord,
  type RunEventEnvelope
} from "./ui-model";

function buildRunDetail(): IpcRunDetail {
  return {
    run: {
      id: 11,
      promptbook_name: "demo",
      promptbook_version: "1.0.0",
      status: "running",
      started_at: "2026-02-21T00:00:00Z",
      finished_at: null,
      agent_default: "dry-run",
      metadata_json: null,
      model: null,
      effort_level: null
    },
    steps: [
      {
        id: 1,
        run_id: 11,
        step_id: "step-1",
        title: "Step one",
        status: "queued",
        started_at: null,
        finished_at: null
      }
    ],
    logs: [],
    outputs: [
      {
        id: 1,
        run_id: 11,
        step_id: "step-1",
        ts: "2026-02-21T00:00:02Z",
        content: "FINAL: first",
        format: "text/plain"
      },
      {
        id: 2,
        run_id: 11,
        step_id: "step-2",
        ts: "2026-02-21T00:00:03Z",
        content: "FINAL: second",
        format: "text/plain"
      }
    ]
  };
}

describe("ui model", () => {
  it("sorts runs by started_at descending", () => {
    const runs: IpcRunRecord[] = [
      {
        id: 1,
        promptbook_name: "old",
        promptbook_version: "1.0.0",
        status: "success",
        started_at: "2026-02-20T00:00:00Z",
        finished_at: null,
        agent_default: null,
        metadata_json: null,
        model: null,
        effort_level: null
      },
      {
        id: 2,
        promptbook_name: "new",
        promptbook_version: "1.1.0",
        status: "running",
        started_at: "2026-02-21T00:00:00Z",
        finished_at: null,
        agent_default: null,
        metadata_json: null,
        model: null,
        effort_level: null
      }
    ];

    const list = createRunListViewModel(runs);
    expect(list[0]?.id).toBe(2);
    expect(list[1]?.id).toBe(1);
  });

  it("returns empty detail model when no run is selected", () => {
    const viewModel = createRunDetailViewModel(null, null);
    expect(viewModel.stepRows).toEqual([]);
    expect(viewModel.liveLines).toEqual([]);
    expect(viewModel.selectedOutput).toBeNull();
  });

  it("keeps selected output null when a non-existent step is selected", () => {
    const viewModel = createRunDetailViewModel(buildRunDetail(), "missing-step");
    expect(viewModel.selectedOutput).toBeNull();
    expect(createRunDetailViewModel(buildRunDetail(), null).selectedOutput?.stepId).toBe("step-1");
    expect(viewModel.outputOptions.map((item) => item.stepId)).toEqual(["step-1", "step-2"]);
  });

  it("applies progress events and ignores empty progress lines", () => {
    const base = buildRunDetail();
    const withLine = applyRunEvent(base, {
      run_id: 11,
      step_id: "step-1",
      type: "step_progress_line",
      payload: {
        line: "working",
        stream: "stderr",
        ts: "2026-02-21T00:00:10Z"
      }
    });
    expect(withLine.logs).toHaveLength(1);
    expect(withLine.logs[0]?.line).toBe("working");

    const ignored = applyRunEvent(withLine, {
      run_id: 11,
      step_id: "step-1",
      type: "step_progress_line",
      payload: {
        line: ""
      }
    });
    expect(ignored.logs).toHaveLength(1);
  });

  it("updates existing steps and adds unknown steps on step_started", () => {
    const base = buildRunDetail();
    const existingStepEvent: RunEventEnvelope = {
      run_id: 11,
      step_id: "step-1",
      type: "step_started",
      payload: { ts: "2026-02-21T00:00:01Z" }
    };
    const existingUpdated = applyRunEvent(base, existingStepEvent);
    expect(existingUpdated.steps[0]?.status).toBe("running");

    const newStep = applyRunEvent(existingUpdated, {
      run_id: 11,
      step_id: "step-2",
      type: "step_started",
      payload: { title: "Step two", ts: "2026-02-21T00:00:04Z" }
    });
    expect(newStep.steps.some((step) => step.step_id === "step-2")).toBe(true);
  });

  it("updates step and run status events only for matching run IDs", () => {
    const base = buildRunDetail();
    const finishedStep = applyRunEvent(base, {
      run_id: 11,
      step_id: "step-1",
      type: "step_finished",
      payload: { status: "success", ts: "2026-02-21T00:00:20Z" }
    });
    expect(finishedStep.steps[0]?.status).toBe("success");
    expect(finishedStep.steps[0]?.finished_at).toBe("2026-02-21T00:00:20Z");

    const finishedRun = applyRunEvent(finishedStep, {
      run_id: 11,
      type: "run_finished",
      payload: { status: "success", ts: "2026-02-21T00:00:21Z" }
    });
    expect(finishedRun.run.status).toBe("success");
    expect(finishedRun.run.finished_at).toBe("2026-02-21T00:00:21Z");

    const unchanged = applyRunEvent(finishedRun, {
      run_id: 999,
      type: "run_finished",
      payload: { status: "failure" }
    });
    expect(unchanged).toBe(finishedRun);
  });
});
