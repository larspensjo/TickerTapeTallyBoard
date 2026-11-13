# Market index spike

**Date:** 2026-11-13
**Scope:** whether market index symbols answer the Yahoo chart request the app already
sends for prices, so the Opportunities page can compare a holding's fall with its market.

## Summary

Every candidate index answered, in both ways the symbol can be written in the address, with
closes and a currency the app's parser accepts. The recommendation is one index per listing
market, and only where the index is quoted in the same currency as the listing. Every other
listing market keeps the median of the other holdings.

| Listing market | How the listing is recognised | Index | Yahoo symbol | Currency |
|---|---|---|---|---|
| United States | Yahoo symbol without a suffix | S&P 500 | `^GSPC` | USD |
| Stockholm | Yahoo suffix `.ST`; otherwise a Nasdaq Nordic mapping in SEK; otherwise instrument currency SEK | OMX Stockholm all-share | `^OMXSPI` | SEK |
| Amsterdam | Yahoo suffix `.AS` | AEX | `^AEX` | EUR |
| Other euro-quoted European listings | Yahoo suffix of Germany (`.DE .F .BE .DU .HM .HA .MU .SG`), France (`.PA`), Belgium (`.BR`), Italy (`.MI`), Spain (`.MC`), Portugal (`.LS`), Austria (`.VI`) or Ireland (`.IR`); otherwise instrument currency EUR with no Nasdaq Nordic mapping | STOXX Europe 600 | `^STOXX` | EUR |

**Keeps the median:** Oslo, Copenhagen, Helsinki and Iceland (no index was probed, because
nothing is held there), and every listing whose currency differs from any chosen index:
London, Switzerland, the non-euro European suffixes, Canada, Australia, Hong Kong, Japan and
unknown suffixes.

The mapping and the index names shown on the page belong to the app; Yahoo's own names
("OMX Stockholm_PI", "STXE 600 I") are not display names.

Things to know about these choices:

- **Price indexes.** All four exclude dividends, like the page's close-to-close change. For
  `^STOXX` this rests on its name and level, not on a stated source.
- **Same currency only.** `^STOXX` is quoted in EUR. Comparing it with a listing in pounds,
  francs or zloty would mix exchange-rate moves into "fell more than its market", so those
  listings keep the median.
- **A listing known only by its euro currency** (no Yahoo symbol) cannot be placed in a
  country and gets `^STOXX`, even if it trades in Amsterdam.
- **The AEX is narrow.** It holds about 25 large companies. Stockholm's all-share index was
  chosen over its 30-company index for breadth; no Amsterdam all-share symbol was probed. A
  holding that is itself a heavy AEX member is partly compared with itself, the weakness
  the median already admits. The S&P 500 has a milder form of this for its largest members.
- **`^OMX`** (OMX Stockholm 30) also answers. It is a candidate to switch to by hand if
  `^OMXSPI` stops answering, not a second symbol the app tries by itself; when an index is
  unavailable the app falls back to the median.

## How it was run

`scripts/probe-market-index.ps1` sent 10 requests on 2026-11-13 at about 14:02 CET, four
seconds apart, with no key and the app's user agent. None failed, none was retried and
nothing suggested throttling. The probe opens no ledger. Its raw responses and summary go
to `.local/logs/`, which is not versioned.

Each of `^GSPC`, `^OMXSPI`, `^OMX`, `^STOXX` and `^AEX` was requested twice with the app's
query (`period1`, `period2`, `interval=1d`, `events=div,splits`, the range ending at the
end of today UTC):

- with the symbol as the app's address builder writes a path segment, over 400 days;
- with the symbol fully percent-encoded, over the 37-day window of a latest refresh.

## Findings

**The `^` in the symbol.** The app's address builder does not percent-encode `^` in a path
segment, so today it sends `/chart/^GSPC`; the app's HTTP library accepts `^` in a path.
Yahoo answered HTTP 200 with identical closes for both `^GSPC` and `%5EGSPC`, for all five
symbols. No change to the address builder is needed. A unit test should pin the form the
app sends, so a library upgrade that starts encoding `^` is noticed.

**What the parser needs.** Every response carried `meta.currency` (the app rejects a
response without one), `meta.symbol` equal to the requested symbol, `meta.gmtoffset`, and
no null closes. `meta.instrumentType` is `INDEX`. No response carried dividend or split
events, so an index series never has events to store.

**Dates.** Mapping each bar to a date the way the app does (the UTC date of timestamp plus
`gmtoffset`) gave no weekend dates and no duplicates in any series. `gmtoffset` is the
market's current offset and is applied to every bar, so summer bars land one hour earlier
in local time (08:30 for New York, 08:00 for the European markets). They stay on the right
date, because bars are stamped at the session open, far from midnight.

**Trading days and gaps.** Over 400 days the series held 276 closes (`^AEX` 281). The
missing weekdays match each market's holidays: ten for New York, eleven for Stockholm, six
for Amsterdam. The longest run of missing weekdays was three (Stockholm over Christmas;
`^STOXX` over Christmas and again over New Year), inside the five-weekday limit the page
uses for price history.

**The STOXX calendar is not the euro markets' calendar.** Yahoo lists `^STOXX` under
Zurich, and its gaps follow that calendar. It had no bar on five days when Amsterdam
traded: 24 and 31 December 2025, 2 January, 14 May and 25 May 2026. The index can therefore
be one or two sessions behind a euro-quoted holding. An index close must be paired with a
holding's close on or before the holding's date, within the page's start-close tolerance,
never by assuming the same date exists. Whether those gaps are the index's real calendar or
only Yahoo's feed was not established.

**The latest bar.** At the run time the European markets were open and New York had not
opened. The four European series ended with a bar for the run day whose close equalled
`meta.regularMarketPrice`, the latest value Yahoo had (about 15 minutes old for `^STOXX`
and `^AEX`). `^GSPC` ended with the previous day's final close. Index series therefore
behave like the app's price series during market hours, and the index store needs the same
rule: a refresh that covers a date overwrites the stored value, so a later refresh replaces
an intraday value with the final close.

**A stale index.** Pairing "on or before" would also pair a fresh holding close with an
index whose refresh failed days ago. The pairing must keep the end side within the same
tolerance as the start side, and otherwise fall back to the median.

**History depth.** The 400-day request returned the whole range, starting on the first
requested day. `meta.firstTradeDate` for `^GSPC` lies in 1927. The market comparison needs
only the selected window plus the start-close tolerance, so depth is not a constraint.

**The two request ranges agree.** Every date in the 37-day response was present in the
400-day response with the same close.

## Limits of this probe

- It used `curl`, not the app's own HTTP client. The address, query, user agent, timeouts
  and protocol version (HTTP/1.1) match the app's. The TLS library differs, `curl` follows
  no redirects where the app follows up to ten, and `curl` honours proxy environment
  variables where the app does not. **Confirmed the same day through the app itself:** one
  latest refresh on a scratch copy of a ledger backup fetched the three indexes its
  holdings needed (`^GSPC`, `^OMXSPI`, `^AEX`) with the app's own client and the unencoded
  `^`, stored 27 or 28 closes for each, reported no index failure and left the refresh
  status as it was (succeeded, no failed items). The copy was deleted afterwards.
- It is one run on one day. An index that answers today can be withdrawn later; the app
  must treat a missing or failing index as "no index for this market" and fall back to the
  median.
- Index symbols come from the same unofficial endpoint as prices and fall under the same
  terms re-check before any distributed installer (see `docs/DecisionLog.md`, 2026-06-13
  "Primary Market Data Sources").

## Fixtures

Recorded from the 37-day responses and trimmed to the fields the parser and the mapping
read (`currency`, `symbol`, `exchangeName`, `fullExchangeName`, `instrumentType`,
`gmtoffset`, `exchangeTimezoneName`, `longName`, timestamps and closes). Each holds 27
final closes from 7 October to 12 November 2026; the bar of the run day was left out of the
three European fixtures. They contain no holiday gap, no calendar difference and no
intraday bar, so tests of pairing, fallback and overwriting need synthetic series.

- `backend/tests/fixtures/market_data/yahoo_index_gspc.json`
- `backend/tests/fixtures/market_data/yahoo_index_omxspi.json`
- `backend/tests/fixtures/market_data/yahoo_index_aex.json`
- `backend/tests/fixtures/market_data/yahoo_index_stoxx.json`
