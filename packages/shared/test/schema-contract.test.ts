import { expectTypeOf } from "vitest";

import {
  ipcRunPromptbookRequestSchema,
  ipcRunPromptbookResponseSchema,
  loadPromptbookFromYaml,
  parseYamlToObject,
  promptbookDefaultsSchema,
  promptbookMetadataSchema,
  promptbookSchema,
  promptbookStepSchema,
  type IpcRunPromptbookRequest,
  type IpcRunPromptbookResponse,
  type Promptbook,
  type PromptbookDefaults,
  type PromptbookMetadata,
  type PromptbookStep
} from "../src/index.ts";

const VALID_PROMPTBOOK_YAML = `
schema_version: "promptbook/v1"
name: "contract-fixture"
version: "1.0.0"
description: "schema contract fixture"
defaults:
  agent: "codex"
  timeout_minutes: 15
  workspace_dir: "."
  approval_mode: "auto"
steps:
  - id: "step-1"
    title: "First"
    prompt: "Do work"
    verify:
      - "echo ok"
metadata:
  tags: ["contract"]
  created_at: "2026-02-21T00:00:00Z"
`;

describe("shared schema contracts", () => {
  it("parses yaml text into object", () => {
    const parsed = parseYamlToObject("name: hello\nsteps: []");
    expect(parsed).toEqual({
      name: "hello",
      steps: []
    });
  });

  it("validates defaults schema (valid + invalid)", () => {
    expect(
      promptbookDefaultsSchema.safeParse({
        agent: "codex",
        timeout_minutes: 30,
        workspace_dir: ".",
        approval_mode: "auto"
      }).success
    ).toBe(true);

    expect(
      promptbookDefaultsSchema.safeParse({
        timeout_minutes: 0
      }).success
    ).toBe(false);
  });

  it("validates metadata schema (valid + invalid)", () => {
    expect(
      promptbookMetadataSchema.safeParse({
        tags: ["sample", "fixture"],
        created_at: "2026-02-21T00:00:00Z"
      }).success
    ).toBe(true);

    expect(
      promptbookMetadataSchema.safeParse({
        tags: [""]
      }).success
    ).toBe(false);
  });

  it("validates step schema (valid + invalid)", () => {
    expect(
      promptbookStepSchema.safeParse({
        id: "step-1",
        title: "Step",
        prompt: "Do work",
        verify: ["echo ok"],
        agent: "dry-run"
      }).success
    ).toBe(true);

    expect(
      promptbookStepSchema.safeParse({
        id: "step-1",
        title: "Step",
        prompt: "Do work",
        verify: [""]
      }).success
    ).toBe(false);
  });

  it("validates promptbook schema (valid + invalid)", () => {
    const valid = loadPromptbookFromYaml(VALID_PROMPTBOOK_YAML);
    expect(valid.schema_version).toBe("promptbook/v1");
    expect(valid.steps.length).toBe(1);

    expect(
      promptbookSchema.safeParse({
        schema_version: "promptbook/v2",
        name: "invalid",
        version: "1.0.0",
        description: "invalid schema version",
        steps: [
          {
            id: "step-1",
            title: "Step",
            prompt: "Do work",
            verify: ["echo ok"]
          }
        ]
      }).success
    ).toBe(false);

    expect(
      promptbookSchema.safeParse({
        schema_version: "promptbook/v1",
        name: "empty-steps",
        version: "1.0.0",
        description: "no steps",
        steps: []
      }).success
    ).toBe(false);
  });

  it("returns readable errors for invalid yaml and schema", () => {
    expect(() => loadPromptbookFromYaml("name: [")).toThrow("Invalid YAML");

    expect(
      () =>
        loadPromptbookFromYaml(`
schema_version: "promptbook/v1"
name: "bad"
version: "1.0.0"
description: "missing required verify"
steps:
  - id: "step-1"
    title: "Step"
    prompt: "Do work"
`)
    ).toThrow("Invalid promptbook schema");
  });

  it("validates IPC request/response schemas (valid + invalid)", () => {
    expect(ipcRunPromptbookRequestSchema.safeParse({ promptbookPath: "/tmp/a.yaml" }).success).toBe(
      true
    );
    expect(ipcRunPromptbookRequestSchema.safeParse({ promptbookPath: "" }).success).toBe(false);

    expect(
      ipcRunPromptbookResponseSchema.safeParse({
        runId: "0d1f8381-9ccf-4811-a6f8-33b24a525d95",
        status: "running"
      }).success
    ).toBe(true);
    expect(
      ipcRunPromptbookResponseSchema.safeParse({
        status: "done"
      }).success
    ).toBe(false);
  });

  it("exports correct inferred types", () => {
    const promptbook = loadPromptbookFromYaml(VALID_PROMPTBOOK_YAML);
    const defaults = promptbook.defaults;
    const metadata = promptbook.metadata;
    const firstStep = promptbook.steps[0];

    expectTypeOf(promptbook).toMatchTypeOf<Promptbook>();
    expectTypeOf(defaults).toMatchTypeOf<PromptbookDefaults | undefined>();
    expectTypeOf(metadata).toMatchTypeOf<PromptbookMetadata | undefined>();
    expectTypeOf(firstStep).toMatchTypeOf<PromptbookStep>();

    const request: IpcRunPromptbookRequest = { promptbookPath: "/tmp/promptbook.v1.yaml" };
    const response: IpcRunPromptbookResponse = { status: "queued" };

    expect(request.promptbookPath).toContain("promptbook");
    expect(response.status).toBe("queued");
  });
});
