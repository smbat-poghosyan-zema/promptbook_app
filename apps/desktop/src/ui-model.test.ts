import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  type IpcModelInfo,
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
      effort_level: null,
      workspace_dir: null,
      step_count: 0,
      current_step_title: null
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

describe("IpcModelInfo", () => {
  it("IpcModelInfo type is exported", () => {
    const model: IpcModelInfo = {
      id: "claude-sonnet-4-6",
      name: "Claude Sonnet 4.6",
      supports_effort: true,
    };
    expect(model.supports_effort).toBe(true);
  });
});

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
        effort_level: null,
        workspace_dir: null,
        step_count: 0,
        current_step_title: null
      },
      {
        id: 2,
        promptbook_name: "new",
        promptbook_version: "1.1.0",
        status: "running",
        started_at: "2026-02-21T00:00:00Z",
        finished_at: null,
        agent_default: "claude",
        metadata_json: null,
        model: "claude-sonnet-4-6",
        effort_level: "medium",
        workspace_dir: "/home/user/project",
        step_count: 3,
        current_step_title: "Generate output"
      }
    ];

    const list = createRunListViewModel(runs);
    expect(list[0]?.id).toBe(2);
    expect(list[1]?.id).toBe(1);
    // Check richer fields on the first item
    expect(list[0]?.title).toBe("new");
    expect(list[0]?.subtitle).toBe("v1.1.0");
    expect(list[0]?.agentLine).toBe("claude / claude-sonnet-4-6 / medium");
    expect(list[0]?.workspaceLine).toBe("/home/user/project");
    expect(list[0]?.stepInfo).toContain("Generate output");
    expect(list[0]?.stepInfo).toContain("3 steps");
    // Item with no workspace falls back to "."
    expect(list[1]?.workspaceLine).toBe(".");
    expect(list[1]?.stepInfo).toBe("");
  });

  it("returns empty detail model when no run is selected", () => {
    const viewModel = createRunDetailViewModel(null, null);
    expect(viewModel.stepRows).toEqual([]);
    expect(viewModel.activeStep).toBeNull();
  });

  it("sets activeStep to the matching step when activeStepId is given", () => {
    const viewModel = createRunDetailViewModel(buildRunDetail(), "step-1");
    expect(viewModel.activeStep?.stepId).toBe("step-1");
    expect(viewModel.activeStep?.finalOutput).toBe("FINAL: first");
  });

  it("falls back to first step when activeStepId is null", () => {
    const viewModel = createRunDetailViewModel(buildRunDetail(), null);
    expect(viewModel.activeStep?.stepId).toBe("step-1");
  });

  it("falls back to first step when activeStepId does not match any step", () => {
    const viewModel = createRunDetailViewModel(buildRunDetail(), "missing-step");
    expect(viewModel.activeStep?.stepId).toBe("step-1");
  });

  it("marks the step after the running step as 'next'", () => {
    const detail: IpcRunDetail = {
      ...buildRunDetail(),
      steps: [
        { id: 1, run_id: 11, step_id: "step-1", title: "Step one", status: "running", started_at: null, finished_at: null },
        { id: 2, run_id: 11, step_id: "step-2", title: "Step two", status: "pending", started_at: null, finished_at: null }
      ]
    };
    const viewModel = createRunDetailViewModel(detail, "step-1");
    expect(viewModel.stepRows[0]?.status).toBe("running");
    expect(viewModel.stepRows[1]?.status).toBe("next");
  });

  it("shows liveLines only for the active step", () => {
    const detail: IpcRunDetail = {
      ...buildRunDetail(),
      logs: [
        { id: 1, run_id: 11, step_id: "step-1", ts: "t1", stream: "stdout", line: "line from step-1" },
        { id: 2, run_id: 11, step_id: "step-2", ts: "t2", stream: "stderr", line: "line from step-2" }
      ]
    };
    const viewModel = createRunDetailViewModel(detail, "step-1");
    expect(viewModel.activeStep?.liveLines).toEqual(["[stdout] line from step-1"]);
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
