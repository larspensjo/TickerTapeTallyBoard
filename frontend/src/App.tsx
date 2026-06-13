import { useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import { useReducer } from "react";
import packageJson from "../package.json";

const frontendVersion = packageJson.version;

type BoardView = "holdings" | "transactions";

type UiState = {
  boardView: BoardView;
};

type UiAction = { type: "boardViewSelected"; boardView: BoardView };

interface HealthResponse {
  status: string;
  version: string;
  build: {
    package: string;
    profile: string;
  };
}

type HoldingRow = {
  symbol: string;
  exchange: string;
  name: string;
  quantity: string;
  price: string;
  value: string;
  dayChange: string;
  dayChangeDirection: "up" | "down" | "flat";
};

type TransactionRow = {
  id: string;
  date: string;
  type: "Buy" | "Sell" | "Split";
  instrument: string;
  market: string;
  quantity: string;
  value: string;
};

const holdings: HoldingRow[] = [
  {
    symbol: "MSFT",
    exchange: "NASDAQ",
    name: "Microsoft",
    quantity: "24",
    price: "USD 474.12",
    value: "SEK 119,840",
    dayChange: "+1.42%",
    dayChangeDirection: "up",
  },
  {
    symbol: "ASML",
    exchange: "EURONEXT",
    name: "ASML Holding",
    quantity: "7",
    price: "EUR 681.90",
    value: "SEK 53,120",
    dayChange: "-0.38%",
    dayChangeDirection: "down",
  },
  {
    symbol: "NOW",
    exchange: "NYSE",
    name: "ServiceNow",
    quantity: "10",
    price: "USD 1,012.45",
    value: "SEK 106,930",
    dayChange: "0.00%",
    dayChangeDirection: "flat",
  },
];

// Mock rows keep the board shape visible until import-backed data lands.
const transactions: TransactionRow[] = [
  {
    id: "mock-2026-06-12-msft-buy-1",
    date: "2026-06-12",
    type: "Buy",
    instrument: "MSFT",
    market: "NASDAQ",
    quantity: "4",
    value: "SEK 19,980",
  },
  {
    id: "mock-2026-06-10-asml-sell-1",
    date: "2026-06-10",
    type: "Sell",
    instrument: "ASML",
    market: "EURONEXT",
    quantity: "-2",
    value: "SEK -15,420",
  },
  {
    id: "mock-2026-06-06-now-split-1",
    date: "2026-06-06",
    type: "Split",
    instrument: "NOW",
    market: "NYSE",
    quantity: "+8",
    value: "SEK 0",
  },
];

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case "boardViewSelected":
      return { ...state, boardView: action.boardView };
  }
}

async function fetchHealth(): Promise<HealthResponse> {
  const response = await fetch("/api/health");

  if (!response.ok) {
    throw new Error(`Health request failed: ${response.status}`);
  }

  return (await response.json()) as HealthResponse;
}

function healthLabel(healthQuery: ReturnType<typeof useQuery<HealthResponse>>) {
  if (healthQuery.isPending) {
    return "Checking API";
  }

  if (healthQuery.isError) {
    return "API offline";
  }

  return `API ${healthQuery.data.status}`;
}

function directionClass(direction: HoldingRow["dayChangeDirection"]) {
  return direction === "flat" ? "number flat" : `number ${direction}`;
}

export function App() {
  const [uiState, dispatch] = useReducer(uiReducer, {
    boardView: "holdings",
  });
  const healthQuery = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
  });

  return (
    <div className="app-shell">
      <header className="app-bar">
        <a className="brand" href="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </a>

        <nav className="app-nav" aria-label="Primary">
          <a className="active" href="/">
            Board
          </a>
          <a href="/">Import</a>
          <a href="/">Settings</a>
        </nav>

        <div className="app-actions">
          <button
            className="button secondary"
            type="button"
            onClick={() => {
              void healthQuery.refetch();
            }}
            disabled={healthQuery.isFetching}
          >
            <RefreshCw
              aria-hidden="true"
              className={healthQuery.isFetching ? "spin" : undefined}
              size={16}
            />
            <span>Refresh</span>
          </button>
          <button className="button primary" type="button">
            <Plus aria-hidden="true" size={16} />
            <span>Add transaction</span>
          </button>
        </div>
      </header>

      <main className="workspace">
        <section className="totals-band" aria-label="Portfolio summary">
          <div>
            <p className="eyebrow">Portfolio value</p>
            <strong className="total-value">SEK 279,890</strong>
          </div>
          <div className="summary-metrics">
            <span>
              Today <strong className="number up">+0.84%</strong>
            </span>
            <span>
              Holdings <strong className="number">3</strong>
            </span>
            <span>
              Transactions <strong className="number">189</strong>
            </span>
          </div>
        </section>

        <section className="status-strip" aria-label="Development status">
          <span
            className={
              healthQuery.isError ? "status-chip warning" : "status-chip"
            }
          >
            {healthLabel(healthQuery)}
          </span>
          <span className="status-chip">EOD pending</span>
          <span className="status-chip">SEK base</span>
        </section>

        <section className="board-grid">
          <article className="panel ledger-panel">
            <div className="panel-header">
              <div>
                <p className="eyebrow">Workspace</p>
                <h1>Portfolio Board</h1>
              </div>
              <fieldset className="segmented-control">
                <legend className="sr-only">Board view</legend>
                <button
                  className={
                    uiState.boardView === "holdings" ? "active" : undefined
                  }
                  type="button"
                  aria-pressed={uiState.boardView === "holdings"}
                  onClick={() =>
                    dispatch({
                      type: "boardViewSelected",
                      boardView: "holdings",
                    })
                  }
                >
                  Holdings
                </button>
                <button
                  className={
                    uiState.boardView === "transactions" ? "active" : undefined
                  }
                  type="button"
                  aria-pressed={uiState.boardView === "transactions"}
                  onClick={() =>
                    dispatch({
                      type: "boardViewSelected",
                      boardView: "transactions",
                    })
                  }
                >
                  Transactions
                </button>
              </fieldset>
            </div>

            {uiState.boardView === "holdings" ? (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th scope="col">Instrument</th>
                      <th scope="col">Qty</th>
                      <th scope="col">Price</th>
                      <th scope="col">Value</th>
                      <th scope="col">Today</th>
                    </tr>
                  </thead>
                  <tbody>
                    {holdings.map((holding) => (
                      <tr key={`${holding.exchange}-${holding.symbol}`}>
                        <td>
                          <div className="instrument-cell">
                            <strong>{holding.symbol}</strong>
                            <span>{holding.name}</span>
                            <em>{holding.exchange}</em>
                          </div>
                        </td>
                        <td className="number">{holding.quantity}</td>
                        <td className="number">{holding.price}</td>
                        <td className="number">{holding.value}</td>
                        <td
                          className={directionClass(holding.dayChangeDirection)}
                        >
                          {holding.dayChange}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th scope="col">Date</th>
                      <th scope="col">Type</th>
                      <th scope="col">Instrument</th>
                      <th scope="col">Qty</th>
                      <th scope="col">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {transactions.map((transaction) => (
                      <tr key={transaction.id}>
                        <td className="number">{transaction.date}</td>
                        <td>
                          <span className="type-chip">{transaction.type}</span>
                        </td>
                        <td>
                          <div className="instrument-cell compact">
                            <strong>{transaction.instrument}</strong>
                            <em>{transaction.market}</em>
                          </div>
                        </td>
                        <td className="number">{transaction.quantity}</td>
                        <td className="number">{transaction.value}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </article>

          <aside className="panel side-panel" aria-label="API status">
            <div className="panel-header compact">
              <div>
                <p className="eyebrow">API</p>
                <h2>Health</h2>
              </div>
              <span
                className={
                  healthQuery.isError ? "status-dot warning" : "status-dot"
                }
                aria-hidden="true"
              />
            </div>

            <dl className="health-list">
              <div>
                <dt>Status</dt>
                <dd>{healthLabel(healthQuery)}</dd>
              </div>
              <div>
                <dt>Backend</dt>
                <dd>
                  {healthQuery.data?.version ??
                    (healthQuery.isPending ? "checking" : "unavailable")}
                </dd>
              </div>
              <div>
                <dt>Build</dt>
                <dd>{healthQuery.data?.build.profile ?? "unknown"}</dd>
              </div>
              <div>
                <dt>Frontend</dt>
                <dd>{frontendVersion}</dd>
              </div>
            </dl>
          </aside>
        </section>
      </main>
    </div>
  );
}
