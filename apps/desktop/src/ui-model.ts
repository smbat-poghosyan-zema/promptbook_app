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

export type IpcModelInfo = {
  id: string;
  name: string;
  supports_effort: boolean;
};

export type IpcStepRecord = {
  id: number;
  run_id: number;
  step_id: string;
  title: string;
  status: string;
  started_at: string | null;
  finished_at: string | null;
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

export type RunListItemViewModel = {
  id: number;
  title: string;
  subtitle: string;
  agentLine: string;
  workspaceLine: string;
  status: string;
  startedAt: string;
  stepInfo: string;
};

export type RunDetailViewModel = {
  liveProgressTitle: "Live Progress";
  finalOutputTitle: "Final Output";
  stepRows: Array<{
    stepId: string;
    title: string;
    status: string;
  }>;
  liveLines: string[];
  outputOptions: Array<{
    stepId: string;
    label: string;
  }>;
  selectedOutput: {
    stepId: string;
    content: string;
    format: string;
  } | null;
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
        startedAt: formatDateTime(run.started_at),
        stepInfo
      };
    });
}

export function createRunDetailViewModel(
  detail: IpcRunDetail | null,
  selectedOutputStepId: string | null
): RunDetailViewModel {
  if (!detail) {
    return {
      liveProgressTitle: "Live Progress",
      finalOutputTitle: "Final Output",
      stepRows: [],
      liveLines: [],
      outputOptions: [],
      selectedOutput: null
    };
  }

  const stepsById = new Map(detail.steps.map((step) => [step.step_id, step]));
  const outputOptions = detail.outputs.map((output) => ({
    stepId: output.step_id,
    label: stepsById.get(output.step_id)?.title ?? output.step_id
  }));
  const fallbackStepId = detail.outputs[0]?.step_id ?? null;
  const resolvedStepId = selectedOutputStepId ?? fallbackStepId;
  const selectedOutput =
    resolvedStepId === null
      ? null
      : detail.outputs.find((output) => output.step_id === resolvedStepId) ?? null;

  return {
    liveProgressTitle: "Live Progress",
    finalOutputTitle: "Final Output",
    stepRows: detail.steps.map((step) => ({
      stepId: step.step_id,
      title: step.title,
      status: step.status
    })),
    liveLines: detail.logs.map(
      (log) => `${log.ts} [${log.stream}] ${log.step_id}: ${log.line}`
    ),
    outputOptions,
    selectedOutput: selectedOutput
      ? {
          stepId: selectedOutput.step_id,
          content: selectedOutput.content,
          format: selectedOutput.format
        }
      : null
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
          finished_at: null
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
