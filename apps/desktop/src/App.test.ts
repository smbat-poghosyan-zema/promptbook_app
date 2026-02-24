import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string) => {
    if (cmd === "list_agent_models") return Promise.resolve([
      { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6", supports_effort: true }
    ]);
    if (cmd === "cancel_run") return Promise.resolve(true);
    if (cmd === "resume_run") return Promise.resolve(1);
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
