# Repo Instructions

## Workflow
- Build with `cargo build` from `backend/`.
- When a task is complete, run `cargo clippy --all-targets -- -D warnings` and then `cargo fmt` from `backend/`.
- For changes under `frontend/`, run `npm run check` and then `npm run fmt` from that directory. `npm run check` covers both `tsc --noEmit` and Biome lint.
- When implementing changes, document them in EngineeringDiary.md. But first look up Instructions how to use in the beginning of the document.
- When creating complex plans, they should be divided into incremental phases that can be tested.
- The UI displays two version values: frontend from `frontend/package.json` and backend from `backend/Cargo.toml` via `/api/health`. Bump these versions as needed.
- The UI should follow the visual design in docs\VisualDesign.DarkTheme.md.

## Planning & Documentation
- When creating a plan, make it clear how to verify each phase. Point out where external human testing is recommended. Save them to the `docs/plans/` folder unless explicitly told otherwise. Check with the docs\DecisionLog.md. The changes shall be staged but not commited so that they can be reviewed.
- When implementing a plan, surface its open questions or ambiguities before silently resolving them.
- Plans are ephemeral documents, and archived when implemented. Never refer to plans or phases from repository documents.

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
- Use `engine_logging` for backend logging.
- Include enough context in error logs to identify the failing job, URL, or operation.

## Decisions
- Keep `docs/DecisionLog.md` up to date for noteworthy decisions.
- See the How to use section in the beginning.
