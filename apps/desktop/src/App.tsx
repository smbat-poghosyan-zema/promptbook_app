import { type FormEvent, useEffect, useMemo, useState } from "react";
import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  type IpcRunDetail,
  type IpcRunRecord,
  type RunEventEnvelope
} from "./ui-model";

type SplitPane = "live" | "final";
type Toast = {
  id: number;
  message: string;
};

type DashboardForm = {
  promptbookPath: string;
  agent: string;
  workspaceDir: string;
};

type TauriListenerPayload<T> = {
  payload: T;
};

const BUILD_INFO = `v${__APP_VERSION__} (${import.meta.env.MODE})`;

const AGENT_OPTIONS = ["codex", "claude", "copilot", "dry-run"];

const FALLBACK_RUNS: IpcRunRecord[] = [
  {
    id: 101,
    promptbook_name: "build-promptbook-runner",
    promptbook_version: "1.0.0",
    status: "running",
    started_at: "2026-02-21T00:15:35.966Z",
    finished_at: null,
    agent_default: "codex",
    metadata_json: null
  },
  {
    id: 100,
    promptbook_name: "hello-world",
    promptbook_version: "1.0.0",
    status: "success",
    started_at: "2026-02-21T00:10:10.000Z",
    finished_at: "2026-02-21T00:10:30.000Z",
    agent_default: "dry-run",
    metadata_json: null
  }
];

const FALLBACK_RUN_DETAILS: Record<number, IpcRunDetail> = {
  101: {
    run: FALLBACK_RUNS[0],
    steps: [
      {
        id: 1,
        run_id: 101,
        step_id: "step-setup",
        title: "Prepare workspace",
        status: "success",
        started_at: "2026-02-21T00:15:36.200Z",
        finished_at: "2026-02-21T00:15:37.000Z"
      },
      {
        id: 2,
        run_id: 101,
        step_id: "step-ui",
        title: "Implement UI MVP",
        status: "running",
        started_at: "2026-02-21T00:15:37.010Z",
        finished_at: null
      }
    ],
    logs: [
      {
        id: 1,
        run_id: 101,
        step_id: "step-setup",
        ts: "2026-02-21T00:15:36.500Z",
        stream: "stdout",
        line: "workspace prepared"
      },
      {
        id: 2,
        run_id: 101,
        step_id: "step-ui",
        ts: "2026-02-21T00:15:37.500Z",
        stream: "stderr",
        line: "running UI tests..."
      }
    ],
    outputs: [
      {
        id: 1,
        run_id: 101,
        step_id: "step-setup",
        ts: "2026-02-21T00:15:37.010Z",
        content: "Workspace prepared and .promptbook_runs initialized.",
        format: "text"
      }
    ]
  },
  100: {
    run: FALLBACK_RUNS[1],
    steps: [
      {
        id: 3,
        run_id: 100,
        step_id: "step-hello",
        title: "Hello world",
        status: "success",
        started_at: "2026-02-21T00:10:10.100Z",
        finished_at: "2026-02-21T00:10:20.100Z"
      }
    ],
    logs: [
      {
        id: 3,
        run_id: 100,
        step_id: "step-hello",
        ts: "2026-02-21T00:10:15.000Z",
        stream: "stdout",
        line: "hello complete"
      }
    ],
    outputs: [
      {
        id: 2,
        run_id: 100,
        step_id: "step-hello",
        ts: "2026-02-21T00:10:20.100Z",
        content: "Hello from Promptbook Runner.",
        format: "text"
      }
    ]
  }
};

function cloneDetail(detail: IpcRunDetail): IpcRunDetail {
  return {
    run: { ...detail.run },
    steps: detail.steps.map((step) => ({ ...step })),
    logs: detail.logs.map((log) => ({ ...log })),
    outputs: detail.outputs.map((output) => ({ ...output }))
  };
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unexpected error";
}

function resolveTauriInvoke():
  | (<T>(command: string, args?: Record<string, unknown>) => Promise<T>)
  | null {
  if (typeof window === "undefined") {
    return null;
  }
  const tauriWindow = window as Window & {
    __TAURI__?: {
      core?: {
        invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
      };
      tauri?: {
        invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
      };
    };
  };

  if (tauriWindow.__TAURI__?.core?.invoke) {
    return tauriWindow.__TAURI__.core.invoke;
  }
  if (tauriWindow.__TAURI__?.tauri?.invoke) {
    return tauriWindow.__TAURI__.tauri.invoke;
  }
  return null;
}

function resolveTauriListen():
  | (<T>(
      eventName: string,
      handler: (event: TauriListenerPayload<T>) => void
    ) => Promise<() => void>)
  | null {
  if (typeof window === "undefined") {
    return null;
  }
  const tauriWindow = window as Window & {
    __TAURI__?: {
      event?: {
        listen?: <T>(
          eventName: string,
          handler: (event: TauriListenerPayload<T>) => void
        ) => Promise<() => void>;
      };
    };
  };

  return tauriWindow.__TAURI__?.event?.listen ?? null;
}

async function invokeIpc<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = resolveTauriInvoke();
  if (!invoke) {
    throw new Error("Tauri runtime not available");
  }
  return invoke<T>(command, args);
}

function hasTauriRuntime(): boolean {
  return resolveTauriInvoke() !== null;
}

export function App() {
  const [runs, setRuns] = useState<IpcRunRecord[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<number | null>(null);
  const [runDetail, setRunDetail] = useState<IpcRunDetail | null>(null);
  const [selectedOutputStepId, setSelectedOutputStepId] = useState<string | null>(null);
  const [splitPane, setSplitPane] = useState<SplitPane>("live");
  const [dashboard, setDashboard] = useState<DashboardForm>({
    promptbookPath: "",
    agent: "codex",
    workspaceDir: "."
  });
  const [isLoadingRuns, setIsLoadingRuns] = useState(true);
  const [isLoadingRunDetail, setIsLoadingRunDetail] = useState(false);
  const [isStartingRun, setIsStartingRun] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const runList = useMemo(() => createRunListViewModel(runs), [runs]);
  const detailViewModel = useMemo(
    () => createRunDetailViewModel(runDetail, selectedOutputStepId),
    [runDetail, selectedOutputStepId]
  );

  function pushErrorToast(message: string): void {
    setToasts((current) => [
      ...current.slice(-2),
      {
        id: Date.now() + current.length,
        message
      }
    ]);
  }

  function dismissToast(id: number): void {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }

  async function loadRuns(): Promise<void> {
    setIsLoadingRuns(true);
    try {
      if (!hasTauriRuntime()) {
        setRuns(FALLBACK_RUNS);
        return;
      }
      const loaded = await invokeIpc<IpcRunRecord[]>("list_runs");
      setRuns(loaded);
    } catch (error) {
      pushErrorToast(`Failed to load runs: ${getErrorMessage(error)}`);
    } finally {
      setIsLoadingRuns(false);
    }
  }

  async function loadRunDetail(runId: number): Promise<void> {
    setIsLoadingRunDetail(true);
    try {
      if (!hasTauriRuntime()) {
        const fallbackDetail = FALLBACK_RUN_DETAILS[runId];
        setRunDetail(fallbackDetail ? cloneDetail(fallbackDetail) : null);
        return;
      }
      const loaded = await invokeIpc<IpcRunDetail | null>("get_run_detail", {
        runId,
        run_id: runId
      });
      setRunDetail(loaded);
    } catch (error) {
      pushErrorToast(`Failed to load run detail: ${getErrorMessage(error)}`);
      setRunDetail(null);
    } finally {
      setIsLoadingRunDetail(false);
    }
  }

  useEffect(() => {
    void loadRuns();
  }, []);

  useEffect(() => {
    if (runs.length === 0) {
      setSelectedRunId(null);
      setRunDetail(null);
      return;
    }

    const runExists = selectedRunId !== null && runs.some((run) => run.id === selectedRunId);
    if (!runExists) {
      setSelectedRunId(runs[0]?.id ?? null);
    }
  }, [runs, selectedRunId]);

  useEffect(() => {
    if (selectedRunId === null) {
      setRunDetail(null);
      return;
    }
    void loadRunDetail(selectedRunId);
  }, [selectedRunId]);

  useEffect(() => {
    if (!runDetail) {
      setSelectedOutputStepId(null);
      return;
    }
    const selectedExists =
      selectedOutputStepId !== null &&
      runDetail.outputs.some((output) => output.step_id === selectedOutputStepId);
    if (!selectedExists) {
      setSelectedOutputStepId(runDetail.outputs[0]?.step_id ?? null);
    }
  }, [runDetail, selectedOutputStepId]);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return;
    }
    const listen = resolveTauriListen();
    if (!listen) {
      return;
    }

    let stop: (() => void) | null = null;
    let isMounted = true;

    listen<RunEventEnvelope>("run_event", (event) => {
      const runEvent = event.payload;
      setRunDetail((current) => {
        if (!current || current.run.id !== runEvent.run_id) {
          return current;
        }
        return applyRunEvent(current, runEvent);
      });
      if (runEvent.type === "run_finished") {
        void loadRuns();
      }
    })
      .then((unlisten) => {
        if (!isMounted) {
          unlisten();
          return;
        }
        stop = unlisten;
      })
      .catch((error) => {
        pushErrorToast(`Failed to subscribe to run events: ${getErrorMessage(error)}`);
      });

    return () => {
      isMounted = false;
      if (stop) {
        stop();
      }
    };
  }, []);

  async function handlePromptbookPicker(): Promise<void> {
    try {
      if (!hasTauriRuntime()) {
        setDashboard((current) => ({
          ...current,
          promptbookPath: current.promptbookPath || "promptbooks/hello-world.v1.yaml"
        }));
        return;
      }
      const selectedPath = await invokeIpc<string | null>("open_file_picker_for_promptbook");
      if (selectedPath) {
        setDashboard((current) => ({ ...current, promptbookPath: selectedPath }));
      }
    } catch (error) {
      pushErrorToast(`Failed to open file picker: ${getErrorMessage(error)}`);
    }
  }

  async function handleStartRun(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault();

    if (!dashboard.promptbookPath.trim()) {
      pushErrorToast("Promptbook file is required");
      return;
    }
    if (!dashboard.workspaceDir.trim()) {
      pushErrorToast("Workspace directory is required");
      return;
    }

    setIsStartingRun(true);
    try {
      if (!hasTauriRuntime()) {
        const runId = Date.now();
        const now = new Date().toISOString();
        const mockRun: IpcRunRecord = {
          id: runId,
          promptbook_name: dashboard.promptbookPath.split("/").pop() ?? "new-run",
          promptbook_version: "1.0.0",
          status: "running",
          started_at: now,
          finished_at: null,
          agent_default: dashboard.agent,
          metadata_json: null
        };
        setRuns((current) => [mockRun, ...current]);
        setSelectedRunId(runId);
        setRunDetail({
          run: mockRun,
          steps: [],
          logs: [],
          outputs: []
        });
        return;
      }

      const runId = await invokeIpc<number>("start_run", {
        promptbookPath: dashboard.promptbookPath,
        promptbook_path: dashboard.promptbookPath,
        agent: dashboard.agent,
        workspaceDir: dashboard.workspaceDir,
        workspace_dir: dashboard.workspaceDir
      });
      await loadRuns();
      setSelectedRunId(runId);
    } catch (error) {
      pushErrorToast(`Failed to start run: ${getErrorMessage(error)}`);
    } finally {
      setIsStartingRun(false);
    }
  }

  function updateDashboardField<K extends keyof DashboardForm>(
    key: K,
    value: DashboardForm[K]
  ): void {
    setDashboard((current) => ({ ...current, [key]: value }));
  }

  return (
    <main className="app-shell">
      <aside className="sidebar panel">
        <header className="panel-header">
          <h2>Runs</h2>
        </header>
        {isLoadingRuns ? (
          <p className="empty-state">Loading runs...</p>
        ) : runList.length === 0 ? (
          <p className="empty-state">No runs yet. Start one from the dashboard.</p>
        ) : (
          <ul className="run-list" aria-label="runs list">
            {runList.map((run) => (
              <li key={run.id}>
                <button
                  type="button"
                  className={`run-item ${run.status} ${
                    selectedRunId === run.id ? "is-selected" : ""
                  }`}
                  onClick={() => setSelectedRunId(run.id)}
                >
                  <strong>{run.title}</strong>
                  <span>{run.subtitle}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </aside>

      <section className="main-column">
        <section className="panel dashboard">
          <header className="panel-header">
            <h1>Promptbook Runner</h1>
            <span className="build-info">{BUILD_INFO}</span>
          </header>

          <form onSubmit={(event) => void handleStartRun(event)} className="dashboard-form">
            <label>
              Promptbook file
              <div className="field-row">
                <input
                  type="text"
                  value={dashboard.promptbookPath}
                  placeholder="promptbooks/hello-world.v1.yaml"
                  onChange={(event) => updateDashboardField("promptbookPath", event.target.value)}
                />
                <button type="button" onClick={() => void handlePromptbookPicker()}>
                  Browse
                </button>
              </div>
            </label>

            <label>
              Agent
              <select
                value={dashboard.agent}
                onChange={(event) => updateDashboardField("agent", event.target.value)}
              >
                {AGENT_OPTIONS.map((agent) => (
                  <option key={agent} value={agent}>
                    {agent}
                  </option>
                ))}
              </select>
            </label>

            <label>
              Workspace directory
              <input
                type="text"
                value={dashboard.workspaceDir}
                placeholder="."
                onChange={(event) => updateDashboardField("workspaceDir", event.target.value)}
              />
            </label>

            <button type="submit" disabled={isStartingRun}>
              {isStartingRun ? "Starting..." : "Start New Run"}
            </button>
          </form>
        </section>

        <section className="panel run-detail">
          <header className="panel-header">
            <h2>Run Detail</h2>
          </header>

          {selectedRunId === null ? (
            <p className="empty-state">Select a run to inspect steps and output.</p>
          ) : isLoadingRunDetail ? (
            <p className="empty-state">Loading run detail...</p>
          ) : !runDetail ? (
            <p className="empty-state">No detail available for this run yet.</p>
          ) : (
            <>
              <ul className="step-list">
                {detailViewModel.stepRows.length === 0 ? (
                  <li className="empty-state">No steps recorded yet.</li>
                ) : (
                  detailViewModel.stepRows.map((step) => (
                    <li key={step.stepId} className="step-row">
                      <span>{step.title}</span>
                      <span className={`status-pill ${step.status}`}>{step.status}</span>
                    </li>
                  ))
                )}
              </ul>

              <div className="split-tabs" role="tablist" aria-label="run detail panes">
                <button
                  type="button"
                  role="tab"
                  aria-selected={splitPane === "live"}
                  className={splitPane === "live" ? "is-active" : ""}
                  onClick={() => setSplitPane("live")}
                >
                  {detailViewModel.liveProgressTitle}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={splitPane === "final"}
                  className={splitPane === "final" ? "is-active" : ""}
                  onClick={() => setSplitPane("final")}
                >
                  {detailViewModel.finalOutputTitle}
                </button>
              </div>

              <div className={`split-pane-grid ${splitPane === "live" ? "show-live" : "show-final"}`}>
                <section className="pane live-pane">
                  <h3>{detailViewModel.liveProgressTitle}</h3>
                  {detailViewModel.liveLines.length === 0 ? (
                    <p className="empty-state">No live progress lines yet.</p>
                  ) : (
                    <pre>{detailViewModel.liveLines.join("\n")}</pre>
                  )}
                </section>

                <section className="pane final-pane">
                  <h3>{detailViewModel.finalOutputTitle}</h3>
                  {detailViewModel.outputOptions.length === 0 ? (
                    <p className="empty-state">No final outputs yet.</p>
                  ) : (
                    <>
                      <label>
                        Step output
                        <select
                          value={selectedOutputStepId ?? detailViewModel.outputOptions[0]?.stepId}
                          onChange={(event) => setSelectedOutputStepId(event.target.value)}
                        >
                          {detailViewModel.outputOptions.map((option) => (
                            <option key={option.stepId} value={option.stepId}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                      </label>
                      {detailViewModel.selectedOutput ? (
                        <pre>{detailViewModel.selectedOutput.content}</pre>
                      ) : (
                        <p className="empty-state">Select a step to see final output.</p>
                      )}
                    </>
                  )}
                </section>
              </div>
            </>
          )}
        </section>
      </section>

      <div className="toast-stack" aria-live="polite" aria-atomic="true">
        {toasts.map((toast) => (
          <div key={toast.id} role="alert" className="toast error">
            <span>{toast.message}</span>
            <button type="button" onClick={() => dismissToast(toast.id)}>
              Dismiss
            </button>
          </div>
        ))}
      </div>
    </main>
  );
}
