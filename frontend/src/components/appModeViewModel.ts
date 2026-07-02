export interface AppModeViewModel {
  showDemoBadge: boolean;
  canMutate: boolean;
  navItems: { to: string; label: string; end?: boolean }[];
}

export function appModeViewModel(demo: boolean | undefined): AppModeViewModel {
  const canMutate = demo === false;

  return {
    showDemoBadge: demo === true,
    canMutate,
    navItems: [
      { to: "/", label: "Dashboard", end: true },
      { to: "/holdings", label: "Holdings" },
      { to: "/gains", label: "Gains" },
      { to: "/transactions", label: "Transactions" },
      ...(canMutate ? [{ to: "/import", label: "Import" }] : []),
    ],
  };
}
