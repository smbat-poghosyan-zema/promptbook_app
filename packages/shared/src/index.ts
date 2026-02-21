import { z } from "zod";

export const promptbookStepSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  prompt: z.string().min(1)
});

export const promptbookSchema = z.object({
  version: z.literal("v1"),
  name: z.string().min(1),
  steps: z.array(promptbookStepSchema)
});

export const ipcRunPromptbookRequestSchema = z.object({
  promptbookPath: z.string().min(1)
});

export const ipcRunPromptbookResponseSchema = z.object({
  runId: z.string().uuid().optional(),
  status: z.enum(["queued", "running", "completed", "failed"])
});

export type Promptbook = z.infer<typeof promptbookSchema>;
export type PromptbookStep = z.infer<typeof promptbookStepSchema>;
export type IpcRunPromptbookRequest = z.infer<
  typeof ipcRunPromptbookRequestSchema
>;
export type IpcRunPromptbookResponse = z.infer<
  typeof ipcRunPromptbookResponseSchema
>;
