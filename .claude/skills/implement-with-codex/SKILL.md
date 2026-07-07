---
name: implement-with-codex
argument-hint: <task or plan phase to implement> [mention Fable to review with Fable instead of Opus]
description: Implementation workflow where Codex writes the code and Claude reviews it - Codex GPT-5.4-Mini (high effort) implements with a pause-on-question contract that hands control back for user decisions, a Claude implementation-reviewer agent (Opus high by default, Fable high on request) reviews the diff, and Codex GPT-5.5 (high effort) applies the accepted findings. Use whenever the user invokes /implement-with-codex, asks Codex to implement something with a Claude review, or wants to implement a plan phase through the Codex implementation pipeline - even if they just say "have codex build this".
---

# Implement with Codex

The counterpart to `/plan-with-codex`: Codex writes the code, Claude reviews
it, Codex applies the review. Model roles are fixed by contract — do not
substitute:

```text
1. Scope        (main thread, with the user)
2. Implement    (codex:codex-rescue: GPT-5.4-Mini, effort high, write)
     ⟲ pause on open question → relay to user → resume with answers
3. Review       (implementation-reviewer agent: Opus high, or Fable on request)
4. Relay        (findings and questions to the user)
5. Apply review (codex:codex-rescue: GPT-5.5, effort high, write)
6. Verify       (main thread: repo completion checks)
```

The review is **single-shot**: one reviewer run, one apply run, then
verification — no re-review loop.

## Step 0: Pick the review model

The `implementation-reviewer` agent pins `model: opus`, `effort: high`; that is
the default. Only when the user has asked for Fable in this conversation pass
`model: "fable"` in the Agent call. Never choose Fable unprompted and never
ask — silence means Opus. The Codex models are not configurable: GPT-5.4-Mini
implements, GPT-5.5 applies the review.

## Step 1: Scope

Establish exactly what is being implemented before any Codex call:

- If the input is a plan (typically `docs/plans/Plan.*.md`, often from
  `/plan-with-codex`), read it and identify the phase or slice in scope,
  including its verification steps. Implement one testable slice per run.
- If the input is a free task description, restate scope and acceptance
  criteria and get the user's nod.
- Check `git status`. A dirty working tree means step 3 will review
  pre-existing changes mixed with Codex's work — tell the user and let them
  decide (commit/stash first, or accept the mixed review) before proceeding.

## Step 2: Implement (GPT-5.4-Mini, pause-on-question)

Dispatch `codex:codex-rescue` (synchronously) with this template. The flags are
routing controls; keep them exact:

```text
--model gpt-5.4-mini --effort high --wait --write

Implementation task.

Implement: <scope from step 1 — spec text or plan path plus the slice,
named by behavior>
Acceptance criteria: <how the result is verified>
Context: <constraints, key decisions, files involved>

Follow the repo conventions in Agents.md: unidirectional data flow
(input → action → reducer → state → render), pure unit-testable reducers, view
logic in pure selectors/view-models not components, thin entry points, shared
constants kept DRY, engine_logging for backend runtime logging with enough
context, regression tests for bug fixes. Build with `cargo build` from
`backend/`; for changes under `frontend/` run `npm run check` from that
directory.

Pause contract: if you reach a decision that needs the user's judgment — an
ambiguous or conflicting requirement, a hard-to-reverse choice, or a needed
deviation from the plan — STOP implementing at that fork. Do not guess. Leave
the working tree consistent (no half-applied edits), and end your output with:

## Paused: Questions for the user
For each question: what is at stake, the realistic options with their
tradeoffs, and your recommendation if you have one.

Otherwise, when the task is done and the build passes, end your output with:

## Implementation complete
Files changed, what was built, how it was verified, and any follow-ups.
```

**If the output contains "Paused: Questions for the user":** relay each
question to the user — the stakes, the options, and a recommendation. Codex
supplies these; add your own analysis where its explanation is thin, and if
you disagree with its recommendation, say so and why rather than passing it
through silently. Use AskUserQuestion with the recommended option first when
the options are enumerable. Then resume the same Codex session:

```text
--resume --model gpt-5.4-mini --effort high --wait --write

The user answered your questions:
<question → answer, one per line>

Continue the implementation under the same pause contract.
```

This pause/resume loop may repeat as often as genuine questions arise — it is
the one loop this workflow allows. If Codex returns nothing or an error, tell
the user and offer a retry; never quietly continue without an implementation.

## Step 3: Review (Claude, Opus high)

When implementation completes, dispatch the `implementation-reviewer` agent
(synchronously) with the scope and acceptance criteria from step 1 and a note
of any user decisions made during pauses (so the reviewer doesn't flag a
deliberate, user-approved choice as a defect). It reads the diff itself and
runs the build checks as evidence.

## Step 4: Relay findings and questions

Show the user the verdict and ranked findings before anything is applied —
they may veto a finding or downgrade it. If the review contains **Questions
for the user**, handle them exactly like implementation pauses: stakes,
options, recommendation, collect answers. Record vetoed findings so step 5
doesn't apply them.

If the verdict is Approved with no findings, skip step 5 entirely and go to
verification — do not spend a GPT-5.5 call applying nothing.

## Step 5: Apply the review (GPT-5.5)

Dispatch `codex:codex-rescue` with a **fresh** task (not --resume — this is a
different model doing a different job; the review text is its context):

```text
--model gpt-5.5 --effort high --wait --write

Apply code-review findings to the uncommitted changes in the working tree.

Task context: <one-paragraph scope from step 1>
Review findings to apply:
<accepted findings verbatim, including file:line and suggested fixes>
User decisions: <answers from step 4, plus vetoed findings marked "do not
apply">

Fix the findings without expanding scope. Keep the repo conventions in
Agents.md. If a finding cannot be applied as described or needs a judgment
call, use the same pause contract: stop, end output with "## Paused:
Questions for the user" (stakes, options, recommendation per question).
When done, end with "## Fixes applied" listing what changed per finding.
```

A pause here is handled the same way as in step 2, resuming with
`--resume --model gpt-5.5 --effort high --wait --write`.

## Step 6: Verify and wrap up

Run the repo's completion checks yourself (per `Agents.md`): from `backend/`,
`cargo clippy --all-targets -- -D warnings` then `cargo fmt`; for changes under
`frontend/`, `npm run check` then `npm run fmt` from that directory. Report
actual output — if something fails, say so plainly and ask the user how to
proceed rather than looping more Codex calls.

Then report: what was implemented, review verdict and which findings were
applied/vetoed, verification results, and any follow-ups the agents flagged.
Do not commit unless the user asks; if the work completed a plan phase,
mention which project documents the plan says to update (design docs under
`docs/`, `docs/DecisionLog.md` candidates, and any version bumps in
`frontend/package.json` / `backend/Cargo.toml`).
