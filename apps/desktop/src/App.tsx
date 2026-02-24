import { type FormEvent, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  type IpcModelInfo,
  type IpcRunDetail,
  type IpcRunRecord,
  type RunEventEnvelope
} from "./ui-model";

type SplitPane = "live" | "final";
type ToastVariant = "error" | "info";
type Toast = {
  id: number;
  message: string;
  variant: ToastVariant;
};

type DashboardForm = {
  promptbookPath: string;
  agent: string;
  model: string;
  effortLevel: string;
  workspaceDir: string;
};

type IpcSamplePromptbook = {
  id: string;
  title: string;
  path: string;
};

const BUILD_INFO = `v${__APP_VERSION__} (${import.meta.env.MODE})`;

const AGENT_OPTIONS = ["codex", "claude", "copilot", "dry-run"];

function getErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.length > 0) {
    return error;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unexpected error";
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
    model: "",
    effortLevel: "medium",
    workspaceDir: "."
  });
  const [availableModels, setAvailableModels] = useState<IpcModelInfo[]>([]);
  const [isLoadingRuns, setIsLoadingRuns] = useState(true);
  const [isLoadingRunDetail, setIsLoadingRunDetail] = useState(false);
  const [isStartingRun, setIsStartingRun] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [samplePromptbooks, setSamplePromptbooks] = useState<IpcSamplePromptbook[]>([]);
  const [selectedSampleId, setSelectedSampleId] = useState<string>("");

  const runList = useMemo(() => createRunListViewModel(runs), [runs]);
  const detailViewModel = useMemo(
    () => createRunDetailViewModel(runDetail, selectedOutputStepId),
    [runDetail, selectedOutputStepId]
  );

  const selectedModelInfo = availableModels.find((m) => m.id === dashboard.model) ?? null;
  const supportsEffort = selectedModelInfo?.supports_effort ?? false;

  function pushToast(message: string, variant: ToastVariant = "error"): void {
    setToasts((current) => [
      ...current.slice(-2),
      {
        id: Date.now() + current.length,
        message,
        variant
      }
    ]);
  }

  function pushErrorToast(message: string): void {
    pushToast(message, "error");
  }

  function dismissToast(id: number): void {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }

  async function loadRuns(): Promise<void> {
    setIsLoadingRuns(true);
    try {
      const loaded = await invoke<IpcRunRecord[]>("list_runs");
      setRuns(loaded);
    } catch (error) {
      pushErrorToast(`Failed to load runs: ${getErrorMessage(error)}`);
    } finally {
      setIsLoadingRuns(false);
    }
  }

  async function loadSamplePromptbooks(): Promise<void> {
    try {
      const loaded = await invoke<IpcSamplePromptbook[]>("list_sample_promptbooks");
      setSamplePromptbooks(loaded);
    } catch (error) {
      pushErrorToast(`Failed to load sample promptbooks: ${getErrorMessage(error)}`);
    }
  }

  async function loadRunDetail(runId: number): Promise<void> {
    setIsLoadingRunDetail(true);
    try {
      const loaded = await invoke<IpcRunDetail | null>("get_run_detail", {
        runId
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
    void loadSamplePromptbooks();
  }, []);

  useEffect(() => {
    invoke<IpcModelInfo[]>("list_agent_models", { agent: dashboard.agent })
      .then((models) => {
        setAvailableModels(models);
        const currentExists = models.some((m) => m.id === dashboard.model);
        if (!currentExists) {
          setDashboard((d) => ({ ...d, model: models[0]?.id ?? "" }));
        }
      })
      .catch(() => setAvailableModels([]));
  }, [dashboard.agent]);

  useEffect(() => {
    if (samplePromptbooks.length === 0) {
      setSelectedSampleId("");
      return;
    }
    const selectedExists = samplePromptbooks.some((sample) => sample.id === selectedSampleId);
    if (!selectedExists) {
      setSelectedSampleId(samplePromptbooks[0]?.id ?? "");
    }
  }, [samplePromptbooks, selectedSampleId]);

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
    let stop: UnlistenFn | null = null;
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
      const selectedPath = await invoke<string | null>("open_file_picker_for_promptbook");
      if (selectedPath) {
        setDashboard((current) => ({ ...current, promptbookPath: selectedPath }));
      }
    } catch (error) {
      pushErrorToast(`Failed to open file picker: ${getErrorMessage(error)}`);
    }
  }

  async function handleOpenSamplePromptbooksFolder(): Promise<void> {
    try {
      await invoke<string>("open_sample_promptbooks_folder");
    } catch (error) {
      pushErrorToast(`Failed to open sample promptbooks folder: ${getErrorMessage(error)}`);
    }
  }

  function handleImportSamplePromptbook(): void {
    if (samplePromptbooks.length === 0) {
      pushErrorToast("No sample promptbooks available");
      return;
    }

    const selected = samplePromptbooks.find((sample) => sample.id === selectedSampleId);
    if (!selected) {
      pushErrorToast("Select a sample promptbook first");
      return;
    }

    setDashboard((current) => ({ ...current, promptbookPath: selected.path }));
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
      const runId = await invoke<number>("start_run", {
        promptbookPath: dashboard.promptbookPath,
        agent: dashboard.agent,
        model: dashboard.model || null,
        effortLevel: dashboard.effortLevel && supportsEffort ? dashboard.effortLevel : null,
        workspaceDir: dashboard.workspaceDir
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
              <div className="sample-controls">
                <button
                  type="button"
                  className="secondary-action"
                  onClick={() => void handleOpenSamplePromptbooksFolder()}
                >
                  Open sample promptbooks folder
                </button>
                <div className="field-row">
                  <select
                    value={selectedSampleId}
                    onChange={(event) => setSelectedSampleId(event.target.value)}
                    disabled={samplePromptbooks.length === 0}
                  >
                    {samplePromptbooks.length === 0 ? (
                      <option value="">No samples found</option>
                    ) : (
                      samplePromptbooks.map((sample) => (
                        <option key={sample.id} value={sample.id}>
                          {sample.title}
                        </option>
                      ))
                    )}
                  </select>
                  <button
                    type="button"
                    className="secondary-action"
                    disabled={samplePromptbooks.length === 0}
                    onClick={handleImportSamplePromptbook}
                  >
                    Import sample
                  </button>
                </div>
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

            {availableModels.length > 0 && (
              <label>
                Model
                <select
                  value={dashboard.model}
                  onChange={(event) => updateDashboardField("model", event.target.value)}
                >
                  {availableModels.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {supportsEffort && (
              <label>
                Effort level
                <select
                  value={dashboard.effortLevel}
                  onChange={(event) => updateDashboardField("effortLevel", event.target.value)}
                >
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                </select>
              </label>
            )}

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
          <div key={toast.id} role="alert" className={`toast ${toast.variant}`}>
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
