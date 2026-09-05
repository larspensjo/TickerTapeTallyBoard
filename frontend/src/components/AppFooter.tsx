import packageJson from "../../package.json";
import { useHealth } from "../api/queries";
import { appFooterViewModel } from "./appFooterViewModel";

function apiStatusLabel(query: ReturnType<typeof useHealth>) {
  if (query.isPending) {
    return "API checking";
  }

  if (query.isError) {
    return "API offline";
  }

  return `API ${query.data.status} ${query.data.version}`;
}

export function AppFooter() {
  const healthQuery = useHealth();

  const footer = appFooterViewModel(
    healthQuery.isSuccess ? healthQuery.data : undefined,
  );
  return (
    <footer className="app-footer">
      <span>UI {packageJson.version}</span>
      <span>{apiStatusLabel(healthQuery)}</span>
      {footer.modeChip ? (
        <span className={footer.modeChip.className}>
          {footer.modeChip.label}
        </span>
      ) : null}
      {footer.ledger ? (
        <span title={footer.ledger.tooltip}>{footer.ledger.label}</span>
      ) : null}
      <span>Manual entry</span>
      <span>SEK base</span>
    </footer>
  );
}
