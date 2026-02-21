# QA Checklist

Use this checklist before cutting a Linux desktop release.

## Automated checks

- [ ] `pnpm test` passes.
- [ ] `pnpm lint` passes.
- [ ] `pnpm build` passes.
- [ ] No unexpected local diffs after running build/test commands.

## Promptbook and runner behavior

- [ ] Load a valid promptbook (`schema_version: "promptbook/v1"`).
- [ ] Invalid promptbook shows actionable validation errors.
- [ ] Run lifecycle events are visible (`run_started`, `step_started`, `step_finished`, `run_finished`/`run_failed`).
- [ ] Step ordering is deterministic and follows file order.
- [ ] Step failure stops execution and reports clear error details.

## Agent adapter checks

- [ ] `codex` adapter command is generated with non-empty program/args.
- [ ] `claude` adapter command is generated with non-empty program/args.
- [ ] `copilot` adapter command is generated with non-empty program/args.
- [ ] `dry-run` adapter emits progress and final output.
- [ ] Unknown agent names fail with a clear error.

## Security checks

- [ ] Workspace path passed to agents is the expected project directory.
- [ ] No adapter enables broad permissions by default.
- [ ] Prompts and verify commands are reviewed before executing untrusted promptbooks.
- [ ] Logs and stored metadata do not expose secrets.

## Packaging sanity checks

- [ ] Build artifacts are produced for Linux packaging flow.
- [ ] App launches and renders the main dashboard.
- [ ] Can start a run from UI and observe status updates.
- [ ] Run history and step outputs persist and are readable.
