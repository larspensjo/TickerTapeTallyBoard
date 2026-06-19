interface InstrumentCellProps {
  name: string;
  symbol: string;
  exchange: string;
}

export function InstrumentCell({
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
      <strong>{primary}</strong>
      {showSymbol ? <span>{trimmedSymbol}</span> : null}
      {exchange ? <em>{exchange}</em> : null}
    </div>
  );
}
