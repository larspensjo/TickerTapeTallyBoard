export interface AppModeViewModel {
  showDemoBadge: boolean;
  canMutate: boolean;
  navItems: { to: string; label: string; end?: boolean }[];
}

export function appModeViewModel(demo: boolean): AppModeViewModel {
  return {
    showDemoBadge: demo,
    canMutate: !demo,
    navItems: [
      { to: "/", label: "Dashboard", end: true },
      { to: "/holdings", label: "Holdings" },
      { to: "/gains", label: "Gains" },
      { to: "/transactions", label: "Transactions" },
      ...(demo ? [] : [{ to: "/import", label: "Import" }]),
    ],
  };
}
