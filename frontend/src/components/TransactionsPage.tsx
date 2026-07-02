import { useState } from "react";
import {
  useDeleteTransaction,
  useHealth,
  useInstruments,
  useTransactions,
} from "../api/queries";
import { AsyncBoundary } from "./AsyncBoundary";
import { TransactionsTable } from "./TransactionsTable";

export function TransactionsPage() {
  const [filter, setFilter] = useState("");
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const healthQuery = useHealth();
  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const deleteTransaction = useDeleteTransaction();
  const canMutate = healthQuery.data?.demo !== true;

  async function handleDelete(id: number) {
    setDeleteError(null);
    try {
      await deleteTransaction.mutateAsync(id);
    } catch (error) {
      setDeleteError(
        error instanceof Error
          ? error.message
          : "Could not delete transaction.",
      );
    }
  }

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Transactions</h1>
          </div>
        </div>
        <AsyncBoundary
          isPending={transactionsQuery.isPending}
          isError={transactionsQuery.isError}
          isEmpty={(transactionsQuery.data?.length ?? 0) === 0}
          onRetry={() => void transactionsQuery.refetch()}
          emptyMessage="No transactions yet. Add one with the button above."
        >
          <TransactionsTable
            transactions={transactionsQuery.data ?? []}
            instruments={instrumentsQuery.data ?? []}
            filter={filter}
            onFilterChange={setFilter}
            onDelete={canMutate ? (id) => void handleDelete(id) : undefined}
            deletingId={
              deleteTransaction.isPending
                ? (deleteTransaction.variables ?? null)
                : null
            }
            errorMessage={deleteError}
            showActions={canMutate}
          />
        </AsyncBoundary>
      </article>
    </section>
  );
}
