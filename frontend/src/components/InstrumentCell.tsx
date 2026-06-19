import { Link } from "react-router-dom";

interface InstrumentCellProps {
  instrumentId: number;
  name: string;
  symbol: string;
  exchange: string;
}

export function InstrumentCell({
  instrumentId,
  name,
  symbol,
  exchange,
}: InstrumentCellProps) {
  const trimmedName = name.trim();
  const trimmedSymbol = symbol.trim();
  const primary = trimmedName || trimmedSymbol;
  const showSymbol = trimmedSymbol.length > 0 && trimmedSymbol !== primary;

  return (
    <div className="instrument-cell">
      <Link className="instrument-link" to={`/asset/${instrumentId}`}>
        {primary}
      </Link>
      {showSymbol ? <span>{trimmedSymbol}</span> : null}
      {exchange ? <em>{exchange}</em> : null}
    </div>
  );
}
