import { type UseQueryResult, useQuery } from "@tanstack/react-query";
import packageJson from "../../package.json";

interface HealthResponse {
  status: string;
  version: string;
  build: { package: string; profile: string };
}

async function fetchHealth(): Promise<HealthResponse> {
  const response = await fetch("/api/health");

  if (!response.ok) {
    throw new Error(`Health request failed: ${response.status}`);
  }

  return (await response.json()) as HealthResponse;
}

function apiStatusLabel(query: UseQueryResult<HealthResponse, Error>) {
  if (query.isPending) {
    return "API checking";
  }

  if (query.isError) {
    return "API offline";
  }

  return `API ${query.data.status} ${query.data.version}`;
}

export function AppFooter() {
  const healthQuery = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
  });

  return (
    <footer className="app-footer">
      <span>UI {packageJson.version}</span>
      <span>{apiStatusLabel(healthQuery)}</span>
      <span>Manual entry</span>
      <span>SEK base</span>
    </footer>
  );
}
