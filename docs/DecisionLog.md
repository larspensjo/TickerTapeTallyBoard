# Decision Log

Purpose: durable record of deliberate commitments — the source of truth for what the
project has agreed to do going forward.

## How to use
### How to add new entries
- One entry per decision. Decisions are commitments, not summaries of work.
- Implementation summaries and bug-fix postmortems belong in `EngineeringDiary.md`.
  A bug fix earns a decision-log entry only when it produces a durable rule, and the
  rule itself is the entry, not the fix.
- Reversals of prior decisions get their own entry referencing the original.
- A plan in itself, or change thereof, is not a decision until it is committed to.
- Keep entries concise and reference concrete artifacts.
- New entries go to the end of the file.

### When to add new entries
- Architecture commitments
- Technology or library choices
- Naming, structural, or coding conventions adopted project-wide
- Safety and scope boundaries
- Experiment outcomes promoted into accepted design
- Reusable rules derived from incidents
- The decisions are typically updated during planning, not during implementation (unless something unexpected happened).

### How to use the decision log during development
- Do not modify older entries if they were commited.

## Entry Template
```
## YYYY-MM-DD - <decision title>
Decision: <the rule, in present tense>
Context: <why this was decided now>
Consequences: <what this constrains or implies going forward>
```

## 2026-06-12: Phase 0 Planning Decisions
Decision: v1 uses SEK as the base currency, tracks one portfolio, imports securities transactions only, uses integer share quantities, trusts the LAN without authentication, and allows manual dividend entry.
Context: These choices keep the first implementation focused on the complete Sharesight All Trades export and the core ledger/valuation path.
Consequences: Phase 0 and Phase 1 do not need account/cash flows, multi-portfolio schema, fractional quantity support, dividend import, or authentication UI/API work.

## 2026-06-12: Repository Layout
Decision: Use top-level `backend/` and `frontend/` directories without a Rust workspace for v1.
Context: The project currently needs one Rust binary and one TypeScript app. A workspace can be introduced later if a CLI, reusable domain crate, or compile-time pressure justifies it.
Consequences: Backend commands run from `backend/`; frontend commands run from `frontend/`. Domain logic should live under `backend/src/domain/` and remain free of axum, sqlx, HTTP, and provider-specific types.

## 2026-06-12: Private Sharesight Exports
Decision: Raw Sharesight exports must not be committed to the repository.
Context: The export contains private portfolio data.
Consequences: `.gitignore` excludes `docs/AllTradesReport*.csv`, `docs/fixtures/private/`, and `docs/spikes/private/`. Versioned docs may include sanitized aggregate findings only.

## 2026-06-12: Split Import Working Model
Decision: Treat Sharesight split quantity as a quantity delta at the import/ledger layer until verified by the Phase 0 invariant check.
Context: The export contains one split row. The spike will sum quantities for the split holding across the complete export and compare that result with Sharesight's current position.
Consequences: If the invariant confirms delta semantics, the ledger stores the source delta and the engine derives ratios when needed. Price-history and buy-marker logic must account for provider split adjustment behavior.

## 2026-06-12: Price Provider Spike Scope
Decision: Unofficial Yahoo Finance data is acceptable for v1 if the Phase 0 provider spike supports it. The spike should compare candidate providers using one USD asset, `MSFT`, and one EUR asset, `ASML`.
Context: v1 only needs end-of-day data for the complete current export, not realtime market data.
Consequences: Phase 0 can include Yahoo as a serious candidate while still documenting symbol mappings, limitations, and fallback options.
