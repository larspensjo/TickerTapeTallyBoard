import packageJson from "../../package.json";
import { useHealth } from "../api/queries";

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

  return (
    <footer className="app-footer">
      <span>UI {packageJson.version}</span>
      <span>{apiStatusLabel(healthQuery)}</span>
      {healthQuery.data?.demo ? <span>DEMO</span> : null}
      <span>Manual entry</span>
      <span>SEK base</span>
    </footer>
  );
}
