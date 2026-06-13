# Repo Instructions

## Workflow
- Build with `cargo build`.
- When a task is complete, run `cargo clippy --all-targets -- -D warnings` and then `cargo fmt`.
- For changes under `crates/qsf_browser_server/ui/`, run `npm run check` and then `npm run fmt` from that directory. `npm run check` covers both `tsc --noEmit` and Biome lint.
- When implementing changes, document them in EngineeringDiary.md. But first look up Instructions how to use in the beginning of the document.
- When creating complex plans, they should be divided into incremental phases that can be tested.

## Planning & Documentation
- When creating or saving plan documents, always save them to the `docs/plans/` folder unless explicitly told otherwise.
- When implementing a plan, surface its open questions or ambiguities before silently resolving them.
- When adding a feature behind a config flag or threshold, the default values must exercise the new code path.
- When creating a plan, make it clear how to verify each step. Point out where external human testing is recommended.
- When creatign a plan, check with docs\ProjectFrame\ProjectWorkflow.md what documents should be updated, and include that in the plan.

## Architecture
- Preserve the unidirectional data flow: input -> action -> reducer -> state -> render, with side effects isolated and fed back as actions.
- Reducers must stay pure and unit-testable.
- Keep entry points (`main.rs`, `mod.rs` and `lib.rs`) files as thin wrappers only.
- Keep shared constants and behavior DRY; prefer one source of truth over duplicated definitions.
- Name runtime modules after stable behavior or domain concepts, not temporary plan phases or milestones.

## Testing
- Bug fixes should include a regression test when practical.
- Prefer tests of reducer behavior, emitted effects, and public contracts over internal details.
- `use super::*;` is acceptable for tests, but using explicit imports is preferable otherwise.

## Logging
- Use `engine_logging` for runtime logging.
- Include enough context in error logs to identify the failing job, URL, or operation.

## Decisions
- Keep `docs/DecisionLog.md` up to date for noteworthy decisions.
- See the How to use section in the beginning.
