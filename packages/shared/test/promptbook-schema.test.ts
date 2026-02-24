import { fileURLToPath } from "node:url";

import { loadPromptbookFromYamlFile } from "../src/index.ts";

describe("promptbook schema loader", () => {
  it("loads a valid YAML promptbook fixture", () => {
    const validFixturePath = fileURLToPath(
      new URL("./fixtures/valid-promptbook.v1.yaml", import.meta.url)
    );

    const promptbook = loadPromptbookFromYamlFile(validFixturePath);

    expect(promptbook.schema_version).toBe("promptbook/v1");
    expect(promptbook.defaults?.agent).toBe("codex");
    expect(promptbook.steps[1]?.agent).toBe("claude");
    expect(promptbook.metadata?.tags?.[0]).toBe("fixture");
  });

  it("fails with a readable error for a missing step id", () => {
    const invalidFixturePath = fileURLToPath(
      new URL("./fixtures/invalid-missing-step-id.v1.yaml", import.meta.url)
    );

    expect(() => loadPromptbookFromYamlFile(invalidFixturePath)).toThrow(
      "Invalid promptbook schema"
    );
  });

  it("validates sample hello-world promptbook", () => {
    const samplePath = fileURLToPath(
      new URL("../../../sample-promptbooks/hello-world.v1.yaml", import.meta.url)
    );

    const promptbook = loadPromptbookFromYamlFile(samplePath);

    expect(promptbook.name).toBe("hello-world");
    expect(promptbook.steps.length).toBe(2);
  });

  it("validates sample repo-audit promptbook", () => {
    const samplePath = fileURLToPath(
      new URL("../../../sample-promptbooks/repo-audit.v1.yaml", import.meta.url)
    );

    const promptbook = loadPromptbookFromYamlFile(samplePath);

    expect(promptbook.name).toBe("repo-audit");
    expect(promptbook.steps.length).toBe(3);
  });
});
