# Promptbook Format: YAML v1

## Why YAML

YAML is the selected v1 format because it is easier to edit by hand, supports inline comments, and produces clean git diffs for iterative prompt changes.

## Schema Version

Every promptbook must declare:

```yaml
schema_version: "promptbook/v1"
```

This keeps parsing deterministic and gives us a stable upgrade path for future versions.

## Promptbook v1 Fields

Required root fields:

- `schema_version`: literal `"promptbook/v1"`
- `name`: promptbook name
- `version`: promptbook version string
- `description`: human-readable summary
- `steps`: ordered list of executable steps

Optional root fields:

- `defaults`
- `metadata`

`defaults` fields:

- `agent` (optional string)
- `timeout_minutes` (optional integer, minimum 1)
- `workspace_dir` (optional string)
- `approval_mode` (optional string)

`steps[]` fields:

- `id` (required string)
- `title` (required string)
- `prompt` (required string, multiline supported)
- `verify` (required string array of shell commands)
- `agent` (optional string override)

`metadata` fields:

- `tags` (optional string array)
- `created_at` (optional string)

## Example

```yaml
schema_version: "promptbook/v1"
name: "hello-world"
version: "1.0.0"
description: "Minimal promptbook example"
defaults:
  agent: "codex"
  timeout_minutes: 20
steps:
  - id: "step-1"
    title: "Create file"
    prompt: |
      Create HELLO.txt with content hello.
    verify:
      - "test -f HELLO.txt"
metadata:
  tags:
    - "example"
  created_at: "2026-02-21T00:00:00Z"
```
