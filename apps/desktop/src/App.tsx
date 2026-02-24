import { type FormEvent, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  applyRunEvent,
  createRunDetailViewModel,
  createRunListViewModel,
  promptbookParentDir,
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
  const [activeStepId, setActiveStepId] = useState<string | null>(null);
  const [expandedStepIds, setExpandedStepIds] = useState<Set<string>>(new Set());
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
    () => createRunDetailViewModel(runDetail, activeStepId),
    [runDetail, activeStepId]
  );

  const selectedModelInfo = availableModels.find((m) => m.id === dashboard.model) ?? null;
  const currentEffortLevels = selectedModelInfo?.effort_levels ?? [];
  const supportsEffort = currentEffortLevels.length > 0;

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
        const defaultModel = models.find((m) => m.is_default) ?? models[0];
        const currentExists = models.some((m) => m.id === dashboard.model);
        if (!currentExists && defaultModel) {
          setDashboard((d) => ({
            ...d,
            model: defaultModel.id,
            effortLevel: defaultModel.default_effort ?? d.effortLevel,
          }));
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
      setActiveStepId(null);
      return;
    }
    const runningStep = runDetail.steps.find((s) => s.status === "running");
    const fallback = runDetail.steps[0];
    setActiveStepId(runningStep?.step_id ?? fallback?.step_id ?? null);
  }, [runDetail?.run.id]);

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
      if (runEvent.type === "step_started" && runEvent.step_id) {
        setActiveStepId(runEvent.step_id);
      }
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
        const parentDir = promptbookParentDir(selectedPath);
        setDashboard((current) => ({
          ...current,
          promptbookPath: selectedPath,
          workspaceDir: parentDir,
        }));
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

    const parentDir = promptbookParentDir(selected.path);
    setDashboard((current) => ({
      ...current,
      promptbookPath: selected.path,
      workspaceDir: parentDir,
    }));
  }

  function toggleStepExpanded(stepId: string): void {
    setExpandedStepIds((prev) => {
      const next = new Set(prev);
      if (next.has(stepId)) {
        next.delete(stepId);
      } else {
        next.add(stepId);
      }
      return next;
    });
  }

  async function handleStopRun(): Promise<void> {
    if (selectedRunId === null) return;
    try {
      await invoke("cancel_run", { runId: selectedRunId });
      await loadRuns();
    } catch (error) {
      pushErrorToast(`Failed to stop run: ${getErrorMessage(error)}`);
    }
  }

  async function handleResumeRun(): Promise<void> {
    if (selectedRunId === null) return;
    try {
      const newRunId = await invoke<number>("resume_run", { originalRunId: selectedRunId });
      await loadRuns();
      setSelectedRunId(newRunId);
    } catch (error) {
      pushErrorToast(`Failed to resume run: ${getErrorMessage(error)}`);
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
                  <div className="run-item-header">
                    <strong className="run-name">{run.title}</strong>
                    <span className={`status-pill ${run.status}`}>{run.statusLabel}</span>
                  </div>
                  <div className="run-item-meta">
                    <span className="run-version">{run.subtitle}</span>
                  </div>
                  {run.agentLine && (
                    <div className="run-item-meta">
                      <span className="run-agent">{run.agentLine}</span>
                    </div>
                  )}
                  <div className="run-item-meta">
                    <span className="run-workspace" title={run.workspaceLine}>{run.workspaceLine}</span>
                  </div>
                  {run.stepInfo && (
                    <div className="run-item-meta">
                      <span className="run-step">{run.stepInfo}</span>
                    </div>
                  )}
                  <div className="run-item-footer">
                    <span className="run-started">{run.startedAt}</span>
                  </div>
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

            <div className="agent-model-effort-row">
              {/* Agent */}
              <label className="ame-field">
                Agent
                <select
                  value={dashboard.agent}
                  onChange={(event) => updateDashboardField("agent", event.target.value)}
                >
                  {AGENT_OPTIONS.map((agent) => (
                    <option key={agent} value={agent}>{agent}</option>
                  ))}
                </select>
              </label>

              {/* Model — always reserve space even if empty, to avoid layout jump */}
              <label className="ame-field">
                Model
                <select
                  value={dashboard.model}
                  onChange={(event) => {
                    const newModelId = event.target.value;
                    const modelInfo = availableModels.find((m) => m.id === newModelId);
                    setDashboard((d) => ({
                      ...d,
                      model: newModelId,
                      effortLevel: modelInfo?.default_effort ?? d.effortLevel,
                    }));
                  }}
                  disabled={availableModels.length === 0}
                >
                  {availableModels.length === 0 ? (
                    <option value="">— no models —</option>
                  ) : (
                    availableModels.map((model) => (
                      <option key={model.id} value={model.id}>{model.name}</option>
                    ))
                  )}
                </select>
              </label>

              {/* Effort — always render, disable when not supported */}
              <label className="ame-field">
                Effort
                <select
                  value={dashboard.effortLevel}
                  onChange={(event) => updateDashboardField("effortLevel", event.target.value)}
                  disabled={!supportsEffort}
                >
                  {currentEffortLevels.length === 0 ? (
                    <option value="">—</option>
                  ) : (
                    currentEffortLevels.map((level) => (
                      <option key={level.id} value={level.id}>{level.name}</option>
                    ))
                  )}
                </select>
              </label>
            </div>

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
                    <li key={step.stepId}
                        className={`step-row ${step.isActive ? "is-active" : ""} step-status-${step.status}`}>

                      <div className="step-top-row">
                        {/* ── Player button (left) ── */}
                        <div className="step-player">
                          {step.status === "running" ? (
                            <button
                              type="button"
                              className="player-btn stop-btn"
                              title="Stop this step"
                              onClick={() => void handleStopRun()}
                            >
                              ■
                            </button>
                          ) : step.status === "failure" || step.status === "stopped" ? (
                            <button
                              type="button"
                              className="player-btn resume-btn"
                              title="Resume from this step"
                              disabled={runDetail?.run.status === "running"}
                              onClick={() => void handleResumeRun()}
                            >
                              ▶
                            </button>
                          ) : step.status === "success" ? (
                            <span className="player-btn done-indicator" title="Completed">✓</span>
                          ) : (
                            /* pending / queued */
                            <span className="player-btn pending-indicator" title="Queued">▶</span>
                          )}
                        </div>

                        {/* ── Clickable header row: selects output pane ── */}
                        <button
                          type="button"
                          className="step-row-select"
                          onClick={() => {
                            if (!step.isActive) {
                              // First click: only activate, do not expand
                              setActiveStepId(step.stepId);
                            } else if (step.isExpandable) {
                              // Already active: toggle expand
                              toggleStepExpanded(step.stepId);
                            }
                          }}
                          aria-expanded={expandedStepIds.has(step.stepId)}
                          aria-label={`Step ${step.stepNumber}: ${step.title}`}
                        >
                          <span className="step-number-badge">STEP {step.stepNumber}</span>
                          <span className="step-title">{step.title}</span>
                          <span className="step-filler" aria-hidden="true" />
                          <span className={`status-pill ${step.status}`}>{step.statusLabel}</span>
                        </button>
                      </div>

                      {/* ── Expanded prompt ── */}
                      {expandedStepIds.has(step.stepId) && step.prompt && (
                        <div className="step-prompt-expand">
                          <pre className="step-prompt-text">{step.prompt}</pre>
                        </div>
                      )}
                    </li>
                  ))
                )}
              </ul>

              {detailViewModel.activeStep ? (
                <>
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
                      <h3>{detailViewModel.liveProgressTitle} — {detailViewModel.activeStep.title}</h3>
                      {detailViewModel.activeStep.liveLines.length === 0 ? (
                        <p className="empty-state">No live progress yet.</p>
                      ) : (
                        <pre>{detailViewModel.activeStep.liveLines.join("\n")}</pre>
                      )}
                    </section>

                    <section className="pane final-pane">
                      <h3>{detailViewModel.finalOutputTitle} — {detailViewModel.activeStep.title}</h3>
                      {detailViewModel.activeStep.finalOutput === null ? (
                        <p className="empty-state">
                          {detailViewModel.activeStep.status === "running"
                            ? "Step in progress — final output will appear when done."
                            : "No final output for this step."}
                        </p>
                      ) : (
                        <pre>{detailViewModel.activeStep.finalOutput}</pre>
                      )}
                    </section>
                  </div>
                </>
              ) : (
                <p className="empty-state">Select a step to view its output.</p>
              )}
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
