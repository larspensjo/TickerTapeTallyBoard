export interface AppModeViewModel {
  showDemoBadge: boolean;
  canMutate: boolean;
  navItems: { to: string; label: string; end?: boolean }[];
}

export function appModeViewModel(
  mode: "production" | "development" | "demo" | undefined,
): AppModeViewModel {
  const canMutate = mode !== undefined && mode !== "demo";

  return {
    showDemoBadge: mode === "demo",
    canMutate,
    navItems: [
      { to: "/", label: "Dashboard", end: true },
      { to: "/holdings", label: "Holdings" },
      { to: "/rebalance", label: "Rebalance" },
      { to: "/gains", label: "Gains" },
      { to: "/transactions", label: "Transactions" },
      ...(canMutate ? [{ to: "/import", label: "Import" }] : []),
    ],
  };
}
