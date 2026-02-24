# Promptbook Format Choice

## Chosen Format: YAML v1

The MVP uses a versioned YAML format (`version: v1`) for Promptbook documents.

## Why YAML v1

- **Human-readable**: easy to author and review in git.
- **Structured**: maps naturally to step-based workflow definitions.
- **Versionable**: explicit `v1` enables backward-compatible evolution.
- **Interoperable**: broad tooling support in JavaScript/TypeScript and Rust.

## Minimal Shape (v1)

```yaml
version: v1
name: Example Promptbook
steps:
  - id: step-1
    type: prompt
    input: |
      Summarize this text.
```

## Validation Strategy

- Validate against a shared schema before run.
- Reject unknown/ambiguous critical fields.
- Surface validation errors with file path + step id context.

## Forward Compatibility

Future versions (`v2+`) should include migration helpers and keep v1 reader support where feasible.
