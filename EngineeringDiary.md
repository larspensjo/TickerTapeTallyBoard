# Engineering Diary

Chronological "what happened" log: every submitted code change, plus research findings,
planning notes, surprises, and open questions encountered during work. This is Stage 1 of
the project workflow; entries may later be promoted to concept notes, research questions,
experiments, or decisions.

## Instructions how to use
- Add one entry per logical change. A logical change can span several related commits.
- Every code change submitted must be reflected by some diary entry. Non-code activities
  (research, planning, observations, things tried that did not pan out) also belong here.
- Decisions and commitments belong in `DecisionLog.md`, not here.
- Keep entries short and reference concrete artifacts.
- New entries go to the end of the file.
- If a change implements a prior decision, note it in the Refs line.
- Don't reference planning documents. Entries shall stand on their own, even after plans are archived.
- There is no need for entries when meta documents are created. E.g. plans or ideas. Only changes to the application.
- Do not modify older entries if they were commited.

## Entry Template
```
## YYYY-MM-DD - <topic>
<one or two sentence summary>
What changed:
- <bullet>
Observed:
- <bullet>
Open question:
- <bullet>
Refs: <files, commits>; implements: <decision title> (if applicable)
```

## 2026-06-13 - Repo skeleton
Added the initial backend and frontend project skeletons using the chosen top-level `backend/` and `frontend/` layout.
What changed:
- Added a minimal Axum/Tokio backend crate with placeholder domain, API, provider, import, database, and migration paths.
- Added a minimal Vite React TypeScript frontend with TypeScript, Biome, build, check, and format scripts.
- Ignored generated frontend dependency and build-output paths.
Observed:
- `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
- `npm run check`, `npm run fmt`, `npm run build`, and `npm audit` passed from `frontend/`.
- The Vite dev server returned the expected root HTML from `http://127.0.0.1:5173/`.
Open question:
- None.
Refs: `backend/`, `frontend/`, `.gitignore`; implements: Repository Layout.

## 2026-06-13 - VS Code workspace configuration
Adapted copied VS Code launch, task, and editor settings to the top-level backend/frontend layout.
What changed:
- Updated debugger launch settings to build and run the backend binary from `backend/`.
- Updated task commands to run Cargo commands from `backend/` and npm commands from `frontend/`.
- Pointed rust-analyzer at `backend/Cargo.toml` and excluded generated frontend output from editor/search views.
Observed:
- `.vscode` JSON files parse successfully.
- No stale `qsf_app` or `crates/qsf_browser_server/ui` references remain in `.vscode`.
- `cargo build` from `backend/` and `npm run check` from `frontend/` passed.
Open question:
- None.
Refs: `.vscode/launch.json`, `.vscode/settings.json`, `.vscode/tasks.json`.
