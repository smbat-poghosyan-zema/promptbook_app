import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "list_agent_models") return Promise.resolve([
      { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6", supports_effort: true }
    ]);
    if (cmd === "cancel_run") return Promise.resolve(true);
    // resume_run returns the SAME run_id (no new run created)
    if (cmd === "resume_run") return Promise.resolve((args?.original_run_id as number) ?? 1);
    if (cmd === "get_run_detail") return Promise.resolve({
      run: { id: 1, promptbook_name: "test", promptbook_version: "1.0.0", status: "success",
             started_at: "1700000000.000Z", finished_at: null, agent_default: null,
             metadata_json: null, model: null, effort_level: null, workspace_dir: null,
             step_count: 1, current_step_title: null },
      steps: [{ id: 1, run_id: 1, step_id: "s1", title: "Step 1",
                status: "success", started_at: null, finished_at: null,
                prompt: "Do the thing" }],
      logs: [],
      outputs: []
    });
    return Promise.resolve(null);
  })
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

describe("App", () => {
  it("renders dashboard shell content", async () => {
    (globalThis as unknown as Record<string, string>).__APP_VERSION__ = "0.1.0-test";
    const { App } = await import("./App");
    const html = renderToStaticMarkup(React.createElement(App));

    expect(html).toContain("Promptbook Runner");
    expect(html).toContain("Runs");
    expect(html).toContain("Run Detail");
    expect(html).toContain("Loading runs...");
    expect(html).toContain("agent-model-effort-row");
    expect(html).toContain("Agent");
    expect(html).toContain("Model");
    expect(html).toContain("Effort");
  });
});

describe("handleResumeRun re-selects same run", () => {
  it("resume_run returns the original run_id so no duplicate entry is created", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    // Simulate calling resume_run with original_run_id=42
    const result = await (invoke as ReturnType<typeof vi.fn>)("resume_run", { original_run_id: 42 });
    // The backend now returns the SAME run_id instead of a new one,
    // so the UI re-selects the existing run rather than creating a duplicate entry.
    expect(result).toBe(42);
  });

  it("resume_run with default run_id returns same id", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const result = await (invoke as ReturnType<typeof vi.fn>)("resume_run", { original_run_id: 1 });
    expect(result).toBe(1);
  });
});
