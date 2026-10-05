import { Link, NavLink } from "react-router-dom";
import { AppBarActions } from "./AppBarActions";
import type { AppModeViewModel } from "./appModeViewModel";
import type { PriceRefreshController } from "./usePriceRefresh";

function navClass({ isActive }: { isActive: boolean }) {
  return isActive ? "active" : undefined;
}

export function AppBar({
  appMode,
  priceRefresh,
}: {
  appMode: AppModeViewModel;
  priceRefresh: PriceRefreshController;
}) {
  return (
    <header className="app-bar">
      <div className="app-bar-identity">
        <Link className="brand" to="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </Link>
        {appMode.showDemoBadge ? (
          <span className="demo-badge">DEMO</span>
        ) : null}
      </div>
      <nav className="app-nav" aria-label="Primary">
        {appMode.navItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            className={navClass}
          >
            {item.label}
          </NavLink>
        ))}
      </nav>
      <AppBarActions
        canMutate={appMode.canMutate}
        priceRefresh={priceRefresh}
      />
    </header>
  );
}
