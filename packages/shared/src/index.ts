// Shared Promptbook schemas and types
import fs from "node:fs";

import YAML from "yaml";
import { z } from "zod";

export function parseYamlToObject(yamlText: string): unknown {
  return YAML.parse(yamlText);
}

export const promptbookDefaultsSchema = z.object({
  agent: z.string().min(1).optional(),
  timeout_minutes: z.number().int().min(1).optional(),
  workspace_dir: z.string().min(1).optional(),
  approval_mode: z.string().min(1).optional()
});

export const promptbookMetadataSchema = z.object({
  tags: z.array(z.string().min(1)).optional(),
  created_at: z.string().min(1).optional()
});

export const promptbookStepSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  prompt: z.string().min(1),
  verify: z.array(z.string().min(1)),
  agent: z.string().min(1).optional()
});

export const promptbookSchema = z.object({
  schema_version: z.literal("promptbook/v1"),
  name: z.string().min(1),
  version: z.string().min(1),
  description: z.string().min(1),
  defaults: promptbookDefaultsSchema.optional(),
  steps: z.array(promptbookStepSchema).min(1),
  metadata: promptbookMetadataSchema.optional()
});

export function loadPromptbookFromYaml(yamlText: string): Promptbook {
  let parsed: unknown;
  try {
    parsed = parseYamlToObject(yamlText);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Invalid YAML: ${message}`);
  }

  try {
    return promptbookSchema.parse(parsed);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Invalid promptbook schema: ${message}`);
  }
}

export function loadPromptbookFromYamlFile(filePath: string): Promptbook {
  const yamlText = fs.readFileSync(filePath, "utf8");
  return loadPromptbookFromYaml(yamlText);
}

export const ipcRunPromptbookRequestSchema = z.object({
  promptbookPath: z.string().min(1)
});

export const ipcRunPromptbookResponseSchema = z.object({
  runId: z.string().uuid().optional(),
  status: z.enum(["queued", "running", "completed", "failed"])
});

export type PromptbookDefaults = z.infer<typeof promptbookDefaultsSchema>;
export type PromptbookMetadata = z.infer<typeof promptbookMetadataSchema>;
export type Promptbook = z.infer<typeof promptbookSchema>;
export type PromptbookStep = z.infer<typeof promptbookStepSchema>;
export type IpcRunPromptbookRequest = z.infer<
  typeof ipcRunPromptbookRequestSchema
>;
export type IpcRunPromptbookResponse = z.infer<
  typeof ipcRunPromptbookResponseSchema
>;
