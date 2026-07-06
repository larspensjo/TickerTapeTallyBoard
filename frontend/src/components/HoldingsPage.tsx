import { useState } from "react";
import { useHoldings, useUpdateInstrumentConvictions } from "../api/queries";
import { AsyncBoundary } from "./AsyncBoundary";
import { HoldingsTable } from "./HoldingsTable";
import { useAppMode } from "./useAppMode";

export function HoldingsPage() {
  const [filter, setFilter] = useState("");
  const holdingsQuery = useHoldings();
  const { canMutate } = useAppMode();
  const applyConvictions = useUpdateInstrumentConvictions();

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Holdings</h1>
          </div>
        </div>
        <AsyncBoundary
          isPending={holdingsQuery.isPending}
          isError={holdingsQuery.isError}
          isEmpty={(holdingsQuery.data?.length ?? 0) === 0}
          onRetry={() => void holdingsQuery.refetch()}
          emptyMessage="No holdings yet. Add a Buy to get started."
        >
          <HoldingsTable
            holdings={holdingsQuery.data ?? []}
            filter={filter}
            onFilterChange={setFilter}
            canEditConviction={canMutate}
            onApplyConvictions={async (changes) => {
              await applyConvictions.mutateAsync(changes);
            }}
            isApplyingConvictions={applyConvictions.isPending}
            applyError={
              applyConvictions.isError
                ? `Could not apply conviction changes: ${applyConvictions.error.message}`
                : null
            }
          />
        </AsyncBoundary>
      </article>
    </section>
  );
}
