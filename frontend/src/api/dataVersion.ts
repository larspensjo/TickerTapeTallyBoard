/**
 * What snapshot of the backend's data a response belongs to, and what day the
 * backend thinks it is. Every data query names this, so two panels cannot show
 * numbers computed from different moments.
 */
export interface DataVersion {
  data_revision: string;
  valuation_date: string;
  prices_refreshing: boolean;
}

export function versionToken(version: DataVersion): string {
  return `${version.data_revision}@${version.valuation_date}`;
}
