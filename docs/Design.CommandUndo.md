# Command Do/Undo Design

**Status:** Accepted in principle; key questions resolved 2026-06-16 (see `docs/DecisionLog.md`). Not yet an implementation plan.
**Date:** 2026-06-16
**Owner:** Lars

This document describes a general do/undo system for TickerTapeTallyBoard using
the Command design pattern. Its key questions were resolved on 2026-06-16 and the
durable commitments live in `docs/DecisionLog.md`; this document holds the
supporting design detail and is not yet an implementation plan.

## 1. Why This Matters

The app already has a ledger-first model: holdings are derived from transactions,
and every ledger write must leave the affected instrument derivable. That makes
undo more important than in a throwaway UI, but also more constrained. Undo must
not merely reverse a button click in React state; it must be a backend write that
is validated against the same ledger invariants as the original action.

The existing Sharesight import rollback is a useful first example. Import commit
creates a batch of transactions in one SQL transaction, and rollback deletes that
batch only if the remaining ledger still derives. A general command system should
make that pattern reusable for manual transactions, edits, deletes, future price
refreshes, and other mutation workflows.

## 2. Current Shape

Relevant existing behavior:

- The frontend keeps local view/form state in reducers, then calls TanStack Query
  mutations for backend writes.
- Manual entry can create an instrument first, then create a transaction. This is
  currently two backend mutations for one user intent.
- Transaction create, replace, and delete all validate domain rules and ledger
  derivability before writing.
- Import preview is read-only. Import commit and rollback are backend-owned,
  transactional operations.
- The database has `transactions.import_batch_id`, but no general command or
  activity table.

Important implication: a good command design should sit below HTTP handlers and
above repositories. Handlers should parse requests; command handlers should own
the write transaction, domain validation, command history, and undo metadata.

## 3. Goals

- Represent meaningful user intent as commands, not as scattered endpoint side
  effects.
- Make undo durable and backend-validated so it works after refresh and across
  devices on the LAN.
- Keep the transaction ledger as the source of truth for portfolio state.
- Preserve the existing unidirectional data flow: input -> action -> reducer ->
  state -> render, with side effects fed back as actions.
- Compose multi-step user intents into one command, especially "create instrument
  if needed, then create transaction".
- Give the UI a clear recent-activity and undo model without making every screen
  invent its own rollback behavior.
- Make failed or blocked undo understandable: the user should know what changed
  after the command and why undo is not currently possible.

## 4. Non-Goals

- Do not replace the ledger with event sourcing. Commands are a control and UX
  layer; holdings still derive from the ledger.
- Do not add full collaborative editing semantics. SQLite serializes writes, and
  v1 remains a local/LAN app.
- Do not guarantee that every write is undoable. Some future writes, such as
  price-cache refreshes, may be repeatable or stale-data cleanup rather than
  user-editable history.
- Do not store raw private Sharesight export bytes. Import redo instead persists
  normalized row snapshots (decided 2026-06-16); accepting that limited private
  data in the command log, but not the raw file.
- Do not make the frontend command stack authoritative. React state can offer
  optimistic affordances, but the backend owns the truth.

## 5. Vocabulary

- **Command:** A named user intent that can be executed, recorded, and possibly
  undone. Examples: create manual transaction, delete transaction, commit
  Sharesight import.
- **Command record:** The persisted history row for an applied command.
- **Undo payload:** Command-specific data captured during execution that is
  sufficient to attempt undo later.
- **Affected entity:** A row touched by the command, such as a transaction,
  instrument, or import batch.
- **Undo latest:** A global undo action against the most recent applied undoable
  command.
- **Selective undo:** Undoing an older command from history. This is useful, but
  it is harder because later commands may depend on it.

## 6. Recommended Design Stance

Use a persisted command log with command-specific execute and undo handlers.
Implement the first version as enum dispatch in Rust rather than trait objects;
that keeps async/database ownership straightforward while still applying the
Command pattern.

Conceptual flow:

```text
UI intent
  -> command API or existing endpoint wrapper
  -> command service
  -> domain validation
  -> SQL transaction
  -> data writes + command record
  -> query invalidation
  -> render updated state

Undo intent
  -> command service undo
  -> domain validation of inverse write
  -> SQL transaction
  -> data writes + command status update
  -> query invalidation
  -> render updated state
```

The command log should not be used to derive holdings. It records how the data
changed and supports UX/history. The transaction ledger remains the canonical
portfolio model.

## 7. Backend Architecture

Add a command module below `api/` and above `db/`:

```text
backend/src/commands/
  mod.rs
  service.rs
  model.rs
  manual_transaction.rs
  import.rs
```

Suggested responsibilities:

- `api/*` modules parse DTOs and call command service functions.
- `commands/service.rs` starts SQL transactions, records command rows, maps
  errors, and exposes shared undo helpers.
- `commands/manual_transaction.rs` owns create, replace, delete, and undo for
  manual transaction commands.
- `commands/import.rs` wraps Sharesight commit and rollback semantics.
- `domain/*` stays pure and does not know about command records.
- `db/*` remains responsible for SQL rows and helpers, including new
  in-transaction repository functions needed by commands.

Rust implementation sketch:

```rust
pub enum CommandRequest {
    CreateManualTransaction(CreateManualTransaction),
    ReplaceTransaction(ReplaceTransaction),
    DeleteTransaction(DeleteTransaction),
    CommitSharesightImport(CommitSharesightImport),
}

pub enum UndoPayload {
    DeleteCreatedTransaction(DeleteCreatedTransactionUndo),
    RestoreDeletedTransaction(RestoreDeletedTransactionUndo),
    RestoreReplacedTransaction(RestoreReplacedTransactionUndo),
    RollbackImportBatch(RollbackImportBatchUndo),
}

pub struct CommandOutcome<T> {
    pub label: String,
    pub result: T,
    pub undo_payload: Option<UndoPayload>,
    pub affected: Vec<CommandEffect>,
}
```

This keeps commands typed inside Rust while persisting payloads as JSON in
SQLite.

## 8. Persistence Model

Initial schema idea:

```sql
CREATE TABLE commands (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    kind              TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('APPLIED', 'UNDONE')),
    label             TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    undone_at         TEXT,
    client_command_id TEXT UNIQUE,
    client_payload_hash TEXT,
    payload_version   INTEGER NOT NULL,
    payload_json      TEXT NOT NULL,
    undo_payload_json TEXT
);

CREATE TABLE command_effects (
    command_id  INTEGER NOT NULL REFERENCES commands (id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id   INTEGER NOT NULL,
    effect      TEXT NOT NULL CHECK (effect IN ('CREATE', 'UPDATE', 'DELETE')),
    PRIMARY KEY (command_id, entity_type, entity_id, effect)
);

CREATE INDEX idx_commands_created_at ON commands (created_at, id);
CREATE INDEX idx_command_effects_entity ON command_effects (entity_type, entity_id);
```

Notes:

- `client_command_id` lets the frontend safely retry a command after a network
  hiccup without double-applying it; `client_payload_hash` distinguishes a safe
  replay from accidental id reuse with a different payload.
- `payload_version` versions the serialized payload/undo-payload shape so old
  records stay readable as command DTOs evolve. Payloads are serialized as
  explicitly tagged JSON, not default enum serialization.
- `label` is the safe, user-visible summary. JSON payloads may contain private
  notes or imported data details and should not be blindly displayed.
- `command_effects` cascades on command delete so the future general cleanup
  function can prune history without orphaning effect rows.
- `command_effects` supports history filtering and future selective-undo checks.
- Failed validation does not need to create a command record in the first
  version. If attempted-command auditing becomes useful, add a separate
  `command_attempts` table later.
- Redo is additive later: redo branch state (for example `sequence` /
  `superseded_at` columns, or a stack-events table) is intentionally out of the
  first schema, which only commits to preserving payloads. The schema does not
  claim redo readiness beyond that.

## 9. Command Execution Contract

Every undoable command should follow this shape:

1. Parse the request into a typed command payload.
2. Start one SQL transaction.
3. Load required current rows.
4. Validate the domain proposal.
5. Apply all writes.
6. Re-derive every affected ledger inside the same transaction.
7. Capture an undo payload from the actual rows written or replaced.
8. Insert one `commands` row and related `command_effects` rows.
9. Commit.

If any step fails, no data rows and no applied command record should be written.

Undo follows the same principle: it is not a blind inverse patch. It is another
validated backend write, using the stored undo payload and the current database
state. Before applying the inverse, undo must verify that the command's effects
are still present — the current rows still match the command's expected
post-state by id and stored row snapshot/hash. If they no longer match, undo is
rejected with `409 undo_blocked` rather than guessing.

### Idempotency And Retries

`client_command_id` makes a command safe to retry after a lost response:

- A retry whose `client_command_id` already committed returns the original
  command record and result rather than applying the command twice.
- A retry that reuses an existing `client_command_id` with a different payload
  (detected via `client_payload_hash`) is rejected with `409 idempotency_conflict`.
- Rejected validation attempts write no command row, so they reserve no
  `client_command_id` and can be retried freely.

## 10. Initial Command Set

### Create Manual Transaction

This should replace the current frontend-driven sequence of `POST /api/instruments`
followed by `POST /api/transactions` when the user enters a new instrument.

Input can include either an existing `instrument_id` or a `new_instrument`
payload. The first write surface is `POST /api/transactions` extended additively
to accept either field (see Section 12); standalone instrument creation stays a
normal, non-undoable resource mutation. Execution upserts the instrument and
inserts the transaction in one backend SQL transaction.

Undo deletes the created transaction. If the command created a brand-new
instrument and no remaining row references it, undo also deletes that instrument
(decided 2026-06-16); the undo payload records whether the instrument was newly
created versus reused. If the instrument already existed, undo must not modify
it. This deliberately differs from import rollback, which leaves batch-created
empty instruments in place until the future general cleanup function.

### Replace Transaction

Execution stores the old complete transaction row in the undo payload, including
audit/import columns even if the manual edit path leaves some of them untouched.

Undo restores the old row and re-validates both the old and new affected
instruments if the edit moved the transaction between instruments.

### Delete Transaction

Execution stores the complete deleted row in the undo payload.

Undo restores that row with the same transaction `id`. This is important because
ledger ordering currently uses `(trade_date, id)` as the deterministic order. If
undo inserted a new row with a new id, a same-day transaction could move in the
ledger and change derived holdings.

This likely requires a repository helper such as `insert_existing_transaction_in_tx`.

### Commit Sharesight Import

Execution reuses the existing import commit path: parse, plan, duplicate guard,
upsert instruments, insert transactions in CSV order, validate affected ledgers,
and write the import batch. The command payload stores normalized inserted-row
snapshots so undo can verify the batch is unchanged and a later redo can re-apply
the rows without re-reading the file.

Undo first verifies the batch is still intact — every transaction the import
inserted is still present and still matches its stored normalized row snapshot —
and rejects with `409 undo_blocked` if the batch was edited or partially deleted
since the command ran. When the batch matches, undo deletes every transaction in
the batch, re-derives affected ledgers, and deletes the now-empty
`import_batches` row. Created instruments are left in place, aligned with the
decision log. The legacy best-effort `rollback/{batch_id}` path (delete by
`import_batch_id` + re-derive, with no match check) is retained only for batches
created before import became command-backed.

### Non-Undoable Commands

Future commands such as price refresh, FX backfill, or provider metadata sync may
still go through the command service for consistent status and activity history,
but can carry `undo_payload_json = NULL` and `can_undo = false`.

## 11. Undo Semantics

Decided first version (2026-06-16):

- Global Undo targets the most recent applied command with an undo payload.
- A command can be undone exactly once.
- Undo updates the original command from `APPLIED` to `UNDONE`.
- Undo by command id is constrained to the latest applied undoable command in the
  MVP: a request for any other command returns `409 not_latest_undoable`.
- Undo may be rejected with `409 undo_blocked` if the command's effects are no
  longer present (current rows do not match its expected post-state), or with
  `422` plus the existing ledger error code if applying the validated inverse
  would itself violate ledger rules.
- Selective undo from the history view is deferred past the undo MVP. When added
  it must still use the same command-specific preconditions and ledger validation,
  and surface a clear blocked-state explanation.

This keeps the primary UX predictable while leaving room for richer command
history. It also matches the ledger reality: undoing an older buy after later
sells may be impossible without first undoing or editing the dependent rows.

Potential `undo_blocked` detail shape:

```json
{
  "error": {
    "code": "undo_blocked",
    "message": "Undo would make the ledger invalid.",
    "details": {
      "command_id": 42,
      "blocking_transaction_id": 108,
      "reason": "sell_exceeds_position"
    }
  }
}
```

## 12. API Shape

Two migration-friendly options exist.

Option A: keep existing resource endpoints and have them create command records:

```text
POST   /api/transactions
PUT    /api/transactions/{id}
DELETE /api/transactions/{id}
POST   /api/import/sharesight/commit
POST   /api/import/sharesight/rollback/{batch_id}
```

Option B: add explicit command endpoints:

```text
GET  /api/commands/recent?limit=20
POST /api/commands/manual-transaction
POST /api/commands/{id}/undo
POST /api/commands/undo-latest
```

Decided approach (2026-06-16): keep Option A resource endpoints and make them
command-backed. `POST /api/transactions` is extended additively to accept either
an `instrument_id` or a `new_instrument` object, so manual entry with a new
instrument is one atomic command without a new endpoint or a changed response
shape; standalone `POST /api/instruments` remains a normal, non-undoable resource
mutation. Add `GET /api/commands/recent` and `POST /api/commands/undo-latest` for
the new UI. `POST /api/commands/{id}/undo` may exist but, in the MVP, returns
`409 not_latest_undoable` unless the target is the current latest undoable
command. Explicit per-command endpoints wait until command payloads diverge from
existing resource DTOs.

Mutation responses should include command metadata:

```json
{
  "data": { "id": 123, "type": "Buy" },
  "command": {
    "id": 42,
    "label": "Added Buy MSFT x10",
    "can_undo": true
  }
}
```

This response shape is a breaking API change, so a softer first step is to keep
current responses and let the frontend refetch `/api/commands/recent` after each
successful mutation.

## 13. Frontend State Model

The undo stack should be server state, not local reducer state. Add React Query
hooks such as:

```text
useCommandHistory()
useUndoLatestCommand()
useUndoCommand(commandId)
```

On any successful mutation, invalidate:

- `commands`
- `transactions`
- `holdings`
- `instruments` when the command may create or remove an instrument

Local reducers should still control local UI phases: form editing, import
preview, duplicate confirmation, command-history drawer open/closed, toast
visibility, and pending state. They should not infer whether backend undo is
possible from local arrays.

## 14. UX Consequences

Undo changes the feel of the app. It lets the UI be faster and less modal, but
only if the system is honest about what can be undone.

Recommended surfaces:

- A global Undo icon button in the app bar, disabled when no undoable command is
  available. Use a tooltip for the command label.
- A History icon button that opens a recent-activity drawer or panel.
- A short toast after successful commands with the command label and an Undo
  action, for example "Added Buy MSFT x10".
- Import success should keep the existing "Undo this import" affordance, but it
  should call the same command undo path once import commit is command-backed.
- Transaction-row delete can become less scary because immediate undo exists,
  but financial edits should still use clear labels and pending states.
- If undo is blocked, show a warning state with the reason and, when possible, a
  link or focus action for the blocking transaction.

Visual direction should follow the dark-theme system:

- Buttons use existing primary, secondary, outline, and icon-button styles.
- Undo and History are compact toolbar controls, not large explanatory cards.
- Activity history is dense: timestamp, command label, status, and a small action
  area.
- Semantic warning color is reserved for blocked undo or failed commands.
- Do not display raw payload JSON in the UI.

Keyboard behavior:

- A global `Ctrl+Z` binding is useful only when focus is outside text inputs,
  selects, textareas, and contenteditable regions.
- Text-entry undo inside forms must remain native browser behavior.
- `Ctrl+Y` (redo) waits until redo is implemented.

Visible undo/redo stack (decided 2026-06-16):

Blind keyboard undo against a long history is the main human-error risk: a few
stray `Ctrl+Z` presses could unwind major unrelated work. Undo and redo must
therefore always be backed by an explicit, visible stack UI, not just a toast.

- A popup/panel shows the do/undo stack with the current position marked.
- `Ctrl+Z` and `Ctrl+Y` move the position down and up the stack, and the popup
  makes the resulting change obvious before and after it happens.
- The user can select a specific entry in the stack and undo or redo to it
  (selective undo/redo). This is the deferred-but-planned capability that lets a
  user patch up an older mistake without blindly unwinding everything after it.
- Each entry shows the safe command `label`, timestamp, and status; raw payload
  JSON is never displayed.

## 15. Redo

Redo is related, but not free.

For simple commands, redo can re-execute the original payload if current state
still satisfies preconditions. For imports, redo either needs the original file,
the normalized rows, or a decision to make import redo unsupported. Storing raw
CSV in command history would preserve redo power but also stores private export
contents in the database.

Decision (2026-06-16): redo is in the plan but deferred past the undo MVP. The
schema must preserve command payloads so redo can be added without a rebuild.
Import redo re-applies stored normalized row snapshots rather than re-reading the
file; persisting that private import data in the command log is accepted. The
redo affordance (and `Ctrl+Y`) only appears once redo is actually implemented.

## 16. Error And Conflict Cases

Cases the design must handle explicitly:

- Undoing a buy may be blocked by a later sell or split.
- Undoing a sell should restore quantity, but still must preserve ledger order.
- Undoing a delete must restore the original transaction id.
- Undoing an edit must fail if the current row no longer matches the expected
  post-edit row and selective undo is being attempted.
- Undoing an import may be blocked by later manual transactions that depend on
  imported buys, or by the batch no longer matching its stored row snapshots.
- Undoing a command that created an instrument must not delete that instrument if
  another row now references it.
- Undo by command id for anything other than the latest undoable command returns
  `409 not_latest_undoable` in the MVP.
- Duplicate browser submits with the same `client_command_id` return the original
  command record; the same id with a different payload hash returns
  `409 idempotency_conflict`.

## 17. Testing Strategy

Backend tests should cover behavior, not implementation details:

- Create manual transaction with new instrument records one command and follows
  the accepted empty-instrument cleanup behavior on undo.
- Create manual transaction using an existing instrument undoes only the
  transaction.
- Delete transaction undo restores the same id and same ledger ordering.
- Replace transaction undo restores old fields and revalidates both instruments
  when instrument id changes.
- Import commit records a command storing normalized row snapshots; undo deletes
  the batch only when it still matches those snapshots and is otherwise
  `undo_blocked`.
- Import undo is rejected when a dependent manual sell or split exists.
- Undo latest picks the newest applied undoable command.
- Undo by command id for a non-latest command returns `409 not_latest_undoable`.
- Duplicate `client_command_id` retries are idempotent; a reused id with a
  different payload returns `409 idempotency_conflict`.

Frontend verification:

- Command history refetches after create, delete, import commit, and undo.
- Global Undo disabled/enabled state matches backend history.
- Toast Undo calls the same mutation as the app-bar Undo.
- Form input `Ctrl+Z` still edits text instead of triggering global undo.
- Human testing is recommended on desktop and mobile widths because the app bar
  and history drawer are UX-heavy changes.

Standard repo checks after implementation phases:

```text
backend/: cargo build
backend/: cargo clippy --all-targets -- -D warnings
backend/: cargo fmt
frontend/: npm run check
frontend/: npm run fmt
```

## 18. Incremental Implementation Sketch

This is not a full plan, but the work naturally divides into phases that can be
tested independently.

### Phase A: Command Schema And Service Skeleton

Add command tables, typed command models, service boundaries, and read-only
`GET /api/commands/recent`. No existing write behavior changes yet.

Verification: migration tests, command repository tests, backend build/test.

### Phase B: Manual Transaction Commands

Move create, replace, and delete transaction writes behind command handlers.
Introduce a combined manual transaction command that can create a new instrument
and transaction atomically.

Verification: existing transaction API tests still pass; new undo tests cover
create/delete/replace.

### Phase C: Import Command Integration

Wrap Sharesight commit as a command and route import rollback through command
undo where a command record exists. Keep a legacy rollback path if needed for
batches created before this migration.

Verification: existing import tests still pass; command history shows import
commit; undo remains blocked by dependent manual transactions.

### Phase D: Undo UX

Add command history query hooks, app-bar Undo/History controls, success toasts,
and blocked-undo messaging. Keep UI dense and consistent with the dark theme.

Verification: `npm run check`, manual desktop/mobile smoke test, create/delete/
import/undo flows.

### Phase E: Selective Undo And Redo Decisions

Decide whether older history items should be undoable from the activity drawer
and whether redo is worth supporting for v1.

Verification: decision-log update if accepted; targeted tests for any expanded
semantics.

## 19. Alternatives Considered

### Frontend-Only Undo

Fast to build for form state, but wrong for durable ledger writes. It would fail
after refresh, across devices, and for import rollback.

### Snapshot-Based Undo

Store whole-table or whole-database snapshots before each command. This is easy
to conceptualize but too blunt. It obscures user intent, complicates concurrent
changes, and risks reverting unrelated work.

### Operation-Specific Rollback Endpoints

This is the current import model. It works for one feature, but every new mutation
would invent its own history, UI, and blocked-state behavior.

### Persistent Command Log

This is the recommended direction. It is explicit about user intent, supports UI
history, composes multi-step actions, and keeps ledger validation at the center.

## 20. Decision Log Candidates

Logged 2026-06-16 in `docs/DecisionLog.md` (Command-Based Writes And
Backend-Validated Undo; Undo/Redo Scope, UX Safety, And Redo Snapshots; Command
History Retention, Cleanup, And Instrument Removal On Undo). The durable
commitments are:

- User-visible writes are executed through backend command handlers, not direct
  repository calls from API handlers.
- Command history is persisted in SQLite and used for undo/activity UX, but the
  transaction ledger remains the source of truth for holdings.
- Undo is a validated backend write and may be rejected if it would violate the
  ledger-write validity invariant.
- Manual transaction entry with a new instrument is one atomic backend command.
- Transaction delete undo must restore the original transaction id while ledger
  ordering depends on `(trade_date, id)`.

## 21. Resolved Questions

Resolved 2026-06-16 and promoted to `docs/DecisionLog.md`:

- **Undo scope:** The undo MVP ships global "undo latest" only. Selective undo
  from history (pick an older entry and undo/redo it) is planned but deferred.
- **Empty manual instrument on undo:** Undoing a manual command that created a
  brand-new instrument also removes that instrument when the command created it
  and nothing else references it; undo never deletes a pre-existing instrument.
  This intentionally differs from import rollback, which leaves batch-created
  empty instruments in place. A later general cleanup function reconciles both.
- **Retention:** Command records are kept indefinitely for now. The UI bounds
  visibility (recent activity), and a future general cleanup function prunes old
  undo history alongside unused instruments.
- **Audit vs UX state:** The command log is UX/activity state, not a formal audit
  log. Rejected attempts are not recorded as command rows; operational failures
  stay in `engine_logging`.
- **Redo:** Included in the plan but deferred past the undo MVP.
- **Import redo storage:** Import redo re-applies stored normalized row snapshots
  rather than re-reading the file. Persisting that private import data in the
  command log is accepted.
- **API migration:** Legacy resource endpoints keep their current response shapes
  first; the frontend refetches command history after mutations. Command metadata
  is added to responses additively later, never as a breaking change.

Still open for a later phase:

- Exact precondition checks and blocked-state UX for selective undo of older
  history entries.
- When the general cleanup function runs (manual action vs scheduled) and its
  retention window for pruned undo history.
