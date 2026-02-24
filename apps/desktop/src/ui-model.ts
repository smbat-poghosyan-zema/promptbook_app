export type IpcRunRecord = {
  id: number;
  promptbook_name: string;
  promptbook_version: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  agent_default: string | null;
  metadata_json: string | null;
  model: string | null;
  effort_level: string | null;
  workspace_dir: string | null;
  step_count: number;
  current_step_title: string | null;
};

export type EffortLevelInfo = {
  id: string;
  name: string;
};

export type IpcModelInfo = {
  id: string;
  name: string;
  effort_levels: EffortLevelInfo[];
  default_effort: string | null;
  is_default: boolean;
};

export type IpcStepRecord = {
  id: number;
  run_id: number;
  step_id: string;
  title: string;
  status: string;
  started_at: string | null;
  finished_at: string | null;
  prompt: string | null;
};

export type IpcLogRecord = {
  id: number;
  run_id: number;
  step_id: string;
  ts: string;
  stream: string;
  line: string;
};

export type IpcOutputRecord = {
  id: number;
  run_id: number;
  step_id: string;
  ts: string;
  content: string;
  format: string;
};

export type IpcRunDetail = {
  run: IpcRunRecord;
  steps: IpcStepRecord[];
  logs: IpcLogRecord[];
  outputs: IpcOutputRecord[];
};

export type RunEventType =
  | "step_started"
  | "step_progress_line"
  | "step_finished"
  | "run_finished";

export type RunEventEnvelope = {
  run_id: number;
  step_id?: string | null;
  type: RunEventType;
  payload: Record<string, unknown>;
};

export function promptbookParentDir(path: string): string {
  const lastSep = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return lastSep > 0 ? path.slice(0, lastSep) : ".";
}

export function formatRunStatus(status: string): string {
  switch (status) {
    case "running": return "in progress";
    case "stopped":
    case "cancelled": return "stopped";
    case "success": return "success";
    case "failure": return "failure";
    case "pending": return "queued";
    default: return status;
  }
}

export type RunListItemViewModel = {
  id: number;
  title: string;
  subtitle: string;
  agentLine: string;
  workspaceLine: string;
  status: string;          // raw (for CSS class)
  statusLabel: string;     // human-readable (for display)
  startedAt: string;
  stepInfo: string;
};

export type StepViewModel = {
  stepId: string;
  title: string;
  status: string;           // raw internal status
  statusLabel: string;      // human-readable (reuse formatRunStatus)
  stepNumber: number;       // 1-based index
  isActive: boolean;
  liveLines: string[];
  finalOutput: string | null;
  prompt: string | null;
  isExpandable: boolean;    // true if prompt is non-null
};

export type RunDetailViewModel = {
  liveProgressTitle: "Live Progress";
  finalOutputTitle: "Final Output";
  stepRows: StepViewModel[];
  activeStep: StepViewModel | null;
};

function readString(payload: Record<string, unknown>, key: string): string | null {
  const value = payload[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function formatDateTime(ts: string | null): string {
  if (!ts) return "—";
  let date: Date;
  if (/^\d+\.\d+Z$/.test(ts)) {
    date = new Date(parseFloat(ts) * 1000);
  } else {
    date = new Date(ts);
  }
  if (isNaN(date.getTime())) return ts;
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  });
}

export function createRunListViewModel(runs: IpcRunRecord[]): RunListItemViewModel[] {
  return [...runs]
    .sort((left, right) => right.started_at.localeCompare(left.started_at))
    .map((run) => {
      const agent = run.agent_default ?? "—";
      const model = run.model ?? "";
      const effort = run.effort_level ?? "";
      const agentParts = [agent, model, effort].filter(Boolean);
      const stepCount = run.step_count;
      let stepInfo = "";
      if (stepCount > 0) {
        if (run.current_step_title) {
          stepInfo = `${run.current_step_title} (${stepCount} step${stepCount !== 1 ? "s" : ""})`;
        } else {
          stepInfo = `${stepCount} step${stepCount !== 1 ? "s" : ""}`;
        }
      }
      return {
        id: run.id,
        title: run.promptbook_name,
        subtitle: `v${run.promptbook_version}`,
        agentLine: agentParts.join(" / "),
        workspaceLine: run.workspace_dir ?? ".",
        status: run.status,
        statusLabel: formatRunStatus(run.status),
        startedAt: formatDateTime(run.started_at),
        stepInfo
      };
    });
}

export function createRunDetailViewModel(
  detail: IpcRunDetail | null,
  activeStepId: string | null
): RunDetailViewModel {
  if (!detail) {
    return {
      liveProgressTitle: "Live Progress",
      finalOutputTitle: "Final Output",
      stepRows: [],
      activeStep: null
    };
  }

  const logsByStep = new Map<string, IpcLogRecord[]>();
  for (const log of detail.logs) {
    const arr = logsByStep.get(log.step_id) ?? [];
    arr.push(log);
    logsByStep.set(log.step_id, arr);
  }
  const outputByStep = new Map(detail.outputs.map((o) => [o.step_id, o]));

  const runningIdx = detail.steps.findIndex((s) => s.status === "running");
  const nextStepId =
    runningIdx >= 0 && runningIdx + 1 < detail.steps.length
      ? detail.steps[runningIdx + 1].step_id
      : null;

  const stepRows: StepViewModel[] = detail.steps.map((step, idx) => {
    const isNext = step.step_id === nextStepId;
    const displayStatus = isNext ? "next" : step.status;
    const logs = logsByStep.get(step.step_id) ?? [];
    const output = outputByStep.get(step.step_id);
    return {
      stepId: step.step_id,
      title: step.title,
      status: displayStatus,
      statusLabel: formatRunStatus(displayStatus),
      stepNumber: idx + 1,
      isActive: step.step_id === activeStepId,
      liveLines: logs.map((l) => `[${l.stream}] ${l.line}`),
      finalOutput: output?.content ?? null,
      prompt: step.prompt ?? null,
      isExpandable: !!(step.prompt)
    };
  });

  const activeStep = stepRows.find((s) => s.isActive) ?? stepRows[0] ?? null;

  return {
    liveProgressTitle: "Live Progress",
    finalOutputTitle: "Final Output",
    stepRows,
    activeStep
  };
}

function nextLogId(logs: IpcLogRecord[]): number {
  const maxId = logs.reduce((currentMax, item) => Math.max(currentMax, item.id), 0);
  return maxId + 1;
}

function nextStepId(steps: IpcStepRecord[]): number {
  const maxId = steps.reduce((currentMax, item) => Math.max(currentMax, item.id), 0);
  return maxId + 1;
}

export function applyRunEvent(detail: IpcRunDetail, event: RunEventEnvelope): IpcRunDetail {
  if (detail.run.id !== event.run_id) {
    return detail;
  }

  const stepId = event.step_id ?? null;
  if (event.type === "step_progress_line" && stepId) {
    const line = readString(event.payload, "line");
    const stream = readString(event.payload, "stream") ?? "stdout";
    const ts = readString(event.payload, "ts") ?? new Date().toISOString();
    if (!line) {
      return detail;
    }
    return {
      ...detail,
      logs: [
        ...detail.logs,
        {
          id: nextLogId(detail.logs),
          run_id: detail.run.id,
          step_id: stepId,
          ts,
          stream,
          line
        }
      ]
    };
  }

  if (event.type === "step_started" && stepId) {
    const existing = detail.steps.some((step) => step.step_id === stepId);
    if (existing) {
      return {
        ...detail,
        steps: detail.steps.map((step) =>
          step.step_id === stepId ? { ...step, status: "running" } : step
        )
      };
    }

    return {
      ...detail,
      steps: [
        ...detail.steps,
        {
          id: nextStepId(detail.steps),
          run_id: detail.run.id,
          step_id: stepId,
          title: readString(event.payload, "title") ?? stepId,
          status: "running",
          started_at: readString(event.payload, "ts"),
          finished_at: null,
          prompt: null
        }
      ]
    };
  }

  if (event.type === "step_finished" && stepId) {
    const status = readString(event.payload, "status") ?? "success";
    const ts = readString(event.payload, "ts");
    return {
      ...detail,
      steps: detail.steps.map((step) =>
        step.step_id === stepId ? { ...step, status, finished_at: ts } : step
      )
    };
  }

  if (event.type === "run_finished") {
    const status = readString(event.payload, "status") ?? detail.run.status;
    const ts = readString(event.payload, "ts");
    return {
      ...detail,
      run: {
        ...detail.run,
        status,
        finished_at: ts
      }
    };
  }

  return detail;
}
