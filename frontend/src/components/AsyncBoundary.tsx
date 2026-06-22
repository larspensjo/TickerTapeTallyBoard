import type { ReactNode } from "react";

interface Props {
  isPending?: boolean;
  isError?: boolean;
  isEmpty?: boolean;
  onRetry?: () => void;
  errorMessage?: string;
  emptyMessage?: ReactNode;
  children?: ReactNode;
}

export function AsyncBoundary({
  isPending = false,
  isError = false,
  isEmpty = false,
  onRetry,
  errorMessage = "Could not load data.",
  emptyMessage,
  children = null,
}: Props) {
  if (isPending) {
    return (
      <div className="board-state">
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="board-state error">
        <p className="down">{errorMessage}</p>
        {onRetry ? (
          <button type="button" className="button outline" onClick={onRetry}>
            Retry
          </button>
        ) : null}
      </div>
    );
  }

  if (isEmpty && emptyMessage !== undefined) {
    return <div className="board-state muted">{emptyMessage}</div>;
  }

  return <>{children}</>;
}
