import { useHealth } from "../api/queries";
import { appModeViewModel } from "./appModeViewModel";

export function useAppMode() {
  const healthQuery = useHealth();

  return appModeViewModel(
    healthQuery.isSuccess ? healthQuery.data.demo : undefined,
  );
}
