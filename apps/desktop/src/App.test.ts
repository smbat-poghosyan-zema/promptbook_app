import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string) => {
    if (cmd === "list_agent_models") return Promise.resolve([
      { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6", supports_effort: true }
    ]);
    if (cmd === "cancel_run") return Promise.resolve(true);
    if (cmd === "resume_run") return Promise.resolve(1);
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
