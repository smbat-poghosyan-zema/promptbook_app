import { ipcRunPromptbookRequestSchema } from "@promptbook/shared";

describe("shared schema smoke", () => {
  it("parses a minimal run payload", () => {
    const payload = ipcRunPromptbookRequestSchema.parse({
      promptbookPath: "/tmp/hello.v1.yaml"
    });

    expect(payload.promptbookPath).toBe("/tmp/hello.v1.yaml");
  });
});
