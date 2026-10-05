import { Link } from "react-router-dom";

interface Props {
  variant: "elsewhere" | "transactions";
  canMutate: boolean;
}

export function LedgerEntryInvitation({ variant, canMutate }: Props) {
  if (!canMutate) return null;

  return (
    <span className="ledger-entry-invitation">
      {variant === "transactions" ? (
        "Use Add transaction above, or "
      ) : (
        <>
          Add one on the <Link to="/transactions">Transactions page</Link> or{" "}
        </>
      )}
      <Link to="/import">import from Avanza or Sharesight</Link>.
    </span>
  );
}
