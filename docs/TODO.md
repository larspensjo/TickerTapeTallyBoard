# Deferred Follow-ups

This lists deferred follow-ups that are not yet planned.

## Opportunities page: the next calibration replay

Approved by the owner on 2026-11-12 and recorded in the 2026-11-13 decision "The
Opportunities Page Judges Each Dip By Named Evidence Checks And A Rule-Based Verdict".
Not built. Nothing here is due before mid-January 2027.

- The next replay runs not before mid-January 2027, then once per quarter. Each is a new
  dated replay on a fresh scratch copy of a ledger backup, under the conditions in
  `docs/spikes/OpportunityChecksSpike.md` ("Safe execution"): the production ledger is never
  opened, only aggregates are committed, and the copy is deleted afterwards.
- Before that replay, and not earlier: extend the replay tool to measure 60 and 120 trading
  days after each dip beside the existing horizons, with tests, and write the change into
  the pre-registered method in `docs/spikes/OpportunityChecksSpike.md`. Commit the method
  before any run; no 60- or 120-day result is looked at before then.
- The following 60 trading days become the deciding horizon once they pass the sufficiency
  floors; 20 and 120 days are reported beside them. A horizon that does not pass its floors
  is reported as counts only. Until 60 days passes, no threshold changes at all. At most one
  threshold change per replay, through a new decision-log entry.
- The same universe, benchmark, exclusion reasons and suppression floors apply per horizon.
  Outcomes overlap about twelve-fold at 60 days and twenty-four-fold at 120 days with
  weekly sampling, so no significance is claimed at any horizon.
- After each replay, update the note on the Opportunities page that states what the replay
  found.
- The offline analysis tools refuse a ledger whose schema differs from the current one, so
  the scratch copy must come from a backup written by an app that already carries the
  current migrations.

## Opportunities page: further evaluations

Listed only. Not approved and not designed.

- Record each day's verdict per holding from now on. Report timing and the business check
  cannot be replayed historically, so the full verdict has never been evaluated. This needs
  new stored data and its own plan.
- Test "stopped falling" as a hypothesis stated before the next replay. Flagged dips that
  had already stopped falling did better at 7 and 14 days in the first replay; that is not
  grounds to promote it into the verdict.
- Evaluate the company-side checks once a year of the app's own observed figures exists,
  from about November 2027.

## Opportunities page: evidence candidates

Listed only. Not approved and not designed.

- Trading volume.
- Sector or industry.
- Cheaper than the company's own usual valuation (historic P/E or P/S).
- Analyst targets.
- Short interest.
- Insider buying.
- News headlines, and any automated reading of news.

## Opportunities page: market indexes

From `docs/spikes/MarketIndexSpike.md`; none is needed for today's holdings.

- Indexes for Oslo, Copenhagen and Helsinki were not probed; holdings there keep the median
  of the other holdings.
- The AEX is narrow. A broader Amsterdam index was not probed.
- That the STOXX Europe 600 symbol is the price index rests on its name and level, not on a
  source.

## Review findings left as they are

- One corrupt stored value among a holding's evidence inputs fails the whole Opportunities
  request with a logged error, not only that row.
- Stored dividend coverage is not cleared when an instrument's price-source symbol changes.
- The data-readiness example tool prepares its price series separately from the domain
  module the app uses.
