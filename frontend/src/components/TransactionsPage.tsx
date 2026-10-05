import { Plus } from "lucide-react";
import { useRef, useState } from "react";
import {
  useDataVersion,
  useDeleteTransaction,
  useInstruments,
  useTransactions,
} from "../api/queries";
import { AddTransactionForm } from "./AddTransactionForm";
import { AsyncBoundary } from "./AsyncBoundary";
import { TransactionsTable } from "./TransactionsTable";
import { useAppMode } from "./useAppMode";

export function TransactionsPage() {
  const [filter, setFilter] = useState("");
  const [formOpen, setFormOpen] = useState(false);
  const addTransactionButtonRef = useRef<HTMLButtonElement>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const appMode = useAppMode();
  const instrumentsQuery = useInstruments();
  const dataVersionQuery = useDataVersion();
  const transactionsQuery = useTransactions();
  const deleteTransaction = useDeleteTransaction();

  function handleCloseForm() {
    setFormOpen(false);
    addTransactionButtonRef.current?.focus();
  }

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
          {appMode.canMutate ? (
            <button
              ref={addTransactionButtonRef}
              type="button"
              className="button secondary"
              aria-expanded={formOpen}
              onClick={() => setFormOpen((open) => !open)}
            >
              <Plus aria-hidden="true" size={16} />
              <span>Add transaction</span>
            </button>
          ) : null}
        </div>
        {formOpen && appMode.canMutate ? (
          <section
            className="transaction-entry-section"
            aria-label="Add transaction"
          >
            <h2>Add transaction</h2>
            <AddTransactionForm
              instruments={instrumentsQuery.data ?? []}
              tradeDate={dataVersionQuery.data?.valuation_date ?? ""}
              onClose={handleCloseForm}
            />
          </section>
        ) : null}
        <AsyncBoundary
          isPending={transactionsQuery.isPending}
          isError={transactionsQuery.isError}
          isEmpty={(transactionsQuery.data?.length ?? 0) === 0}
          onRetry={() => void transactionsQuery.refetch()}
          emptyMessage={
            appMode.canMutate
              ? "No transactions yet. Use Add transaction above."
              : "No transactions yet."
          }
        >
          <TransactionsTable
            transactions={transactionsQuery.data ?? []}
            instruments={instrumentsQuery.data ?? []}
            filter={filter}
            onFilterChange={setFilter}
            onDelete={
              appMode.canMutate ? (id) => void handleDelete(id) : undefined
            }
            deletingId={
              deleteTransaction.isPending
                ? (deleteTransaction.variables ?? null)
                : null
            }
            errorMessage={deleteError}
            showActions={appMode.canMutate}
          />
        </AsyncBoundary>
      </article>
    </section>
  );
}
