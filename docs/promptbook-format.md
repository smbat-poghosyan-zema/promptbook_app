# Promptbook Format Choice: YAML v1

## Decision

The MVP promptbook format is **YAML v1**.

## Why YAML v1

1. Authoring ergonomics
- YAML is readable for non-programmers and concise for prompt-heavy content.
- Multiline strings are first-class, which helps with longer prompt templates.

2. Versioned contract
- A `version: v1` field allows strict parsing rules now and non-breaking evolution later.
- The runner can branch behavior by explicit version instead of guessing schema intent.

3. Tooling compatibility
- Strong parser support in both Node.js and Rust ecosystems.
- Easy integration with JSON Schema-style validation workflows.

4. Operational simplicity
- Human-diffable files for review in git.
- No database or binary format needed for MVP.

## MVP shape (illustrative)

```yaml
version: v1
name: hello-world
steps:
  - id: greet
    type: prompt
    prompt: |
      Say hello to {{name}}.
    input:
      name: World
```

## Non-goals for v1

- Dynamic plugin step types.
- Complex conditional branching.
- Backward-compatibility for unversioned files.

These can be layered in future versions behind explicit schema upgrades.
