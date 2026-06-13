import { useEffect, useReducer } from "react";
import packageJson from "../package.json";

const frontendVersion = packageJson.version;

type BackendVersionState =
  | { kind: "loading" }
  | { kind: "available"; version: string }
  | { kind: "unavailable" };

type BackendVersionAction =
  | { type: "backendVersionLoaded"; version: string }
  | { type: "backendVersionFailed" };

interface HealthResponse {
  version: string;
}

function backendVersionReducer(
  _state: BackendVersionState,
  action: BackendVersionAction,
): BackendVersionState {
  switch (action.type) {
    case "backendVersionLoaded":
      return { kind: "available", version: action.version };
    case "backendVersionFailed":
      return { kind: "unavailable" };
  }
}

export function App() {
  const [backendVersion, dispatchBackendVersion] = useReducer(
    backendVersionReducer,
    { kind: "loading" },
  );

  useEffect(() => {
    let isCurrent = true;

    async function loadBackendVersion() {
      try {
        const response = await fetch("/api/health");

        if (!response.ok) {
          throw new Error(`Health request failed: ${response.status}`);
        }

        const health = (await response.json()) as HealthResponse;

        if (isCurrent) {
          dispatchBackendVersion({
            type: "backendVersionLoaded",
            version: health.version,
          });
        }
      } catch {
        if (isCurrent) {
          dispatchBackendVersion({ type: "backendVersionFailed" });
        }
      }
    }

    void loadBackendVersion();

    return () => {
      isCurrent = false;
    };
  }, []);

  return (
    <main className="app-shell">
      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">Portfolio tracker</p>
            <h1>TickerTapeTallyBoard</h1>
          </div>
          <span className="status-pill">Skeleton</span>
        </header>

        <div className="summary-grid">
          <article>
            <span>Total value</span>
            <strong>SEK 0</strong>
          </article>
          <article>
            <span>Holdings</span>
            <strong>0</strong>
          </article>
          <article>
            <span>Transactions</span>
            <strong>0</strong>
          </article>
        </div>

        <section className="work-list" aria-labelledby="next-work">
          <h2 id="next-work">Next implementation slices</h2>
          <ul>
            <li>Backend health API</li>
            <li>Frontend API status query</li>
            <li>Sharesight import spike</li>
          </ul>
        </section>

        <footer className="app-footer">
          <span>Frontend {frontendVersion}</span>
          <span aria-hidden="true">/</span>
          <span>
            Backend{" "}
            {backendVersion.kind === "available"
              ? backendVersion.version
              : backendVersion.kind}
          </span>
        </footer>
      </section>
    </main>
  );
}
