import fs from "node:fs";

import { z } from "zod";

function getIndentation(line: string): number {
  let count = 0;
  while (count < line.length && line[count] === " ") {
    count += 1;
  }

  return count;
}

function stripInlineComment(value: string): string {
  let inSingleQuote = false;
  let inDoubleQuote = false;

  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    const prev = index > 0 ? value[index - 1] : "";

    if (char === "'" && !inDoubleQuote) {
      inSingleQuote = !inSingleQuote;
      continue;
    }

    if (char === '"' && !inSingleQuote && prev !== "\\") {
      inDoubleQuote = !inDoubleQuote;
      continue;
    }

    if (char === "#" && !inSingleQuote && !inDoubleQuote) {
      const previousChar = index === 0 ? " " : value[index - 1] ?? " ";
      if (previousChar === " " || previousChar === "\t") {
        return value.slice(0, index).trimEnd();
      }
    }
  }

  return value;
}

function parseScalar(value: string): unknown {
  const trimmed = stripInlineComment(value).trim();

  if (trimmed.length === 0) {
    return "";
  }

  if (trimmed === "true") {
    return true;
  }

  if (trimmed === "false") {
    return false;
  }

  if (trimmed === "null" || trimmed === "~") {
    return null;
  }

  if (/^-?\d+$/.test(trimmed)) {
    return Number(trimmed);
  }

  if (/^-?\d+\.\d+$/.test(trimmed)) {
    return Number(trimmed);
  }

  if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
    return trimmed.slice(1, -1).replace(/\\n/g, "\n").replace(/\\"/g, '"');
  }

  if (trimmed.startsWith("'") && trimmed.endsWith("'")) {
    return trimmed.slice(1, -1).replace(/''/g, "'");
  }

  return trimmed;
}

type ParsedBlock = {
  nextIndex: number;
  value: unknown;
};

function parseBlockScalar(
  lines: string[],
  startIndex: number,
  expectedIndentation: number,
  style: "|" | ">"
): ParsedBlock {
  const collected: string[] = [];
  let index = startIndex;

  while (index < lines.length) {
    const line = lines[index] ?? "";
    const trimmed = line.trim();

    if (trimmed.length === 0) {
      collected.push("");
      index += 1;
      continue;
    }

    const indentation = getIndentation(line);
    if (indentation < expectedIndentation) {
      break;
    }

    collected.push(line.slice(expectedIndentation));
    index += 1;
  }

  if (style === "|") {
    return {
      nextIndex: index,
      value: `${collected.join("\n")}${collected.length > 0 ? "\n" : ""}`
    };
  }

  return {
    nextIndex: index,
    value: collected
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
      .join(" ")
  };
}

function parseKeyValue(content: string): { key: string; rawValue: string } {
  const delimiterIndex = content.indexOf(":");
  if (delimiterIndex <= 0) {
    throw new Error(`Invalid YAML key/value line: ${content}`);
  }

  return {
    key: content.slice(0, delimiterIndex).trim(),
    rawValue: content.slice(delimiterIndex + 1).trim()
  };
}

function parseObject(lines: string[], startIndex: number, indentation: number): ParsedBlock {
  const output: Record<string, unknown> = {};
  let index = startIndex;

  while (index < lines.length) {
    const line = lines[index] ?? "";
    const trimmed = line.trim();

    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      index += 1;
      continue;
    }

    const currentIndentation = getIndentation(line);
    if (currentIndentation < indentation) {
      break;
    }

    if (currentIndentation > indentation) {
      throw new Error(`Unexpected indentation on line ${index + 1}`);
    }

    const content = line.slice(indentation);
    if (content.startsWith("- ")) {
      break;
    }

    const { key, rawValue } = parseKeyValue(content);

    if (rawValue === "" || rawValue === undefined) {
      const nested = parseNode(lines, index + 1, indentation + 2);
      output[key] = nested.value;
      index = nested.nextIndex;
      continue;
    }

    if (rawValue === "|" || rawValue === ">") {
      const block = parseBlockScalar(lines, index + 1, indentation + 2, rawValue);
      output[key] = block.value;
      index = block.nextIndex;
      continue;
    }

    output[key] = parseScalar(rawValue);
    index += 1;
  }

  return { nextIndex: index, value: output };
}

function parseArray(lines: string[], startIndex: number, indentation: number): ParsedBlock {
  const output: unknown[] = [];
  let index = startIndex;

  while (index < lines.length) {
    const line = lines[index] ?? "";
    const trimmed = line.trim();

    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      index += 1;
      continue;
    }

    const currentIndentation = getIndentation(line);
    if (currentIndentation < indentation) {
      break;
    }

    if (currentIndentation > indentation) {
      throw new Error(`Unexpected indentation on line ${index + 1}`);
    }

    const content = line.slice(indentation);
    if (!content.startsWith("- ")) {
      break;
    }

    const itemContent = content.slice(2).trim();
    if (itemContent.length === 0) {
      const nested = parseNode(lines, index + 1, indentation + 2);
      output.push(nested.value);
      index = nested.nextIndex;
      continue;
    }

    if (itemContent.includes(":")) {
      const firstKeyValue = parseKeyValue(itemContent);
      const itemObject: Record<string, unknown> = {};

      if (firstKeyValue.rawValue === "|" || firstKeyValue.rawValue === ">") {
        const block = parseBlockScalar(
          lines,
          index + 1,
          indentation + 4,
          firstKeyValue.rawValue
        );
        itemObject[firstKeyValue.key] = block.value;
        index = block.nextIndex;
      } else if (firstKeyValue.rawValue.length === 0) {
        const nested = parseNode(lines, index + 1, indentation + 4);
        itemObject[firstKeyValue.key] = nested.value;
        index = nested.nextIndex;
      } else {
        itemObject[firstKeyValue.key] = parseScalar(firstKeyValue.rawValue);
        index += 1;
      }

      while (index < lines.length) {
        const nextLine = lines[index] ?? "";
        const nextTrimmed = nextLine.trim();

        if (nextTrimmed.length === 0 || nextTrimmed.startsWith("#")) {
          index += 1;
          continue;
        }

        const nextIndentation = getIndentation(nextLine);
        if (nextIndentation <= indentation) {
          break;
        }

        if (nextIndentation !== indentation + 2) {
          throw new Error(`Unexpected indentation on line ${index + 1}`);
        }

        const nextContent = nextLine.slice(indentation + 2);
        const nextKeyValue = parseKeyValue(nextContent);

        if (nextKeyValue.rawValue === "|" || nextKeyValue.rawValue === ">") {
          const block = parseBlockScalar(lines, index + 1, indentation + 4, nextKeyValue.rawValue);
          itemObject[nextKeyValue.key] = block.value;
          index = block.nextIndex;
          continue;
        }

        if (nextKeyValue.rawValue.length === 0) {
          const nested = parseNode(lines, index + 1, indentation + 4);
          itemObject[nextKeyValue.key] = nested.value;
          index = nested.nextIndex;
          continue;
        }

        itemObject[nextKeyValue.key] = parseScalar(nextKeyValue.rawValue);
        index += 1;
      }

      output.push(itemObject);
      continue;
    }

    output.push(parseScalar(itemContent));
    index += 1;
  }

  return { nextIndex: index, value: output };
}

function parseNode(lines: string[], startIndex: number, indentation: number): ParsedBlock {
  let index = startIndex;

  while (index < lines.length) {
    const line = lines[index] ?? "";
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      index += 1;
      continue;
    }

    const currentIndentation = getIndentation(line);
    if (currentIndentation < indentation) {
      return { nextIndex: index, value: {} };
    }

    if (currentIndentation > indentation) {
      throw new Error(`Unexpected indentation on line ${index + 1}`);
    }

    if (line.slice(indentation).startsWith("- ")) {
      return parseArray(lines, index, indentation);
    }

    return parseObject(lines, index, indentation);
  }

  return { nextIndex: index, value: {} };
}

export function parseYamlToObject(yamlText: string): unknown {
  const lines = yamlText.replace(/\r\n/g, "\n").split("\n");
  return parseNode(lines, 0, 0).value;
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
  steps: z.array(promptbookStepSchema),
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
