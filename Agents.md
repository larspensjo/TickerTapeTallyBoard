# Repository Instructions

Keep guidance focused on durable project constraints and user preferences. Trust
agents to choose implementation details and follow existing conventions.

## Working with the user

The user works across many projects and treats code as a black box. Own technical
investigation, implementation, and verification; explain observable outcomes.

- At task start, during meaningful progress updates, and at completion, give a
  brief status: **TickerTapeTallyBoard — [task]: [current state]. Next: [next step
  or none].** Keep it to one or two plain-language sentences so the user can
  quickly regain context. Avoid narrating tool calls and file edits.
- Final status states what changed, whether it was verified, and any remaining
  blocker or user decision. Distinguish implemented from verified; checks of one
  change do not establish the health of the whole project.
- Keep code details, file lists, and command output out of routine summaries unless
  requested or needed to explain a material risk or decision. Surface uncertainty.
- Make routine, reversible implementation decisions independently. Surface plan
  ambiguities that materially affect behavior, scope, compatibility, or user data;
  explain the user-facing tradeoff and recommend an option. Resolve ordinary
  implementation details using judgment without requiring user code review.

## Project boundaries

- Preserve input -> action -> reducer -> state -> render. Reducers are pure;
  side effects return through actions. View derivation belongs in pure selectors
  or view-models; components consume their results and render.
- Keep entry points and orchestration thin. Name runtime modules after stable
  behavior or domain concepts, not temporary plan phases or milestones.
- Follow `docs/VisualDesign.DarkTheme.md` for UI work.
- The UI displays separate frontend and backend versions: `frontend/package.json`
  and `backend/Cargo.toml`, with the latter exposed through `/api/health`.
  Update the relevant versions when warranted by the change.
- Use `engine_logging` for backend runtime logging, with enough context to identify
  the failing job, URL, or operation.
- Raw Sharesight exports are private and must not be committed. Versioned findings
  must be sanitized, as recorded in `docs/DecisionLog.md`.
- Preserve SQL migration bytes, including their LF line endings; applied migration
  checksums are part of the schema contract. See the decision log before repairs.

## Verification

- Select verification appropriate to the change and report checks that could not
  be completed. Documentation-only edits do not need application builds or tests.
- For backend Rust changes, run `cargo build`, relevant tests,
  `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` from `backend/`.
- For frontend changes, run relevant tests, `npm run check`, and `npm run fmt`
  from `frontend/`. The check covers TypeScript and Biome lint.
- When launching npm through PowerShell `Start-Process`, use `npm.cmd` explicitly;
  bare `npm` can resolve to a script that opens in Notepad.
- Add practical regression coverage for behavior fixes. Prefer reducer, selector,
  effect-layer (`api/client.ts`), and public-contract tests. Use component tests
  by role/text for user-visible behavior that is not covered at those boundaries;
  avoid snapshots and assertions on internal state or DOM structure.

## Planning and project memory

- Consult relevant entries in `docs/DecisionLog.md` before planning. Favor durable
  solutions and divide complex work into testable phases, with clear verification
  and any recommended human testing. Save plans in `docs/plans/` unless instructed
  otherwise.
- Plans and reviews are ephemeral and deleted after external review. Durable
  documents and code name the behavior rather than referring to plan phases.
- Keep affected documentation accurate. Record settled commitments in
  `docs/DecisionLog.md`, following its "How to use" section. Append reversals
  rather than rewriting committed entries; omit routine implementation summaries.
