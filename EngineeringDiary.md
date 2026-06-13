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