import { Link } from "react-router-dom";
import type { Ticker } from "../api/types";

interface InstrumentCellProps {
  instrumentId: number;
  name: string;
  ticker: Ticker;
}

export function InstrumentCell({
  instrumentId,
  name,
  ticker,
}: InstrumentCellProps) {
  return (
    <div className="instrument-cell">
      <Link className="instrument-link" to={`/asset/${instrumentId}`}>
        {name.trim() || ticker || "Unnamed instrument"}
      </Link>
      {ticker ? <span>{ticker}</span> : null}
    </div>
  );
}
