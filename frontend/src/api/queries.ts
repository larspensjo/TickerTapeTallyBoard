import {
  keepPreviousData,
  type UseQueryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { apiGet, apiSend, apiSendBytes, apiSendWithStatus } from "./client";
import { type DataVersion, versionToken } from "./dataVersion";
import { normalizeRebalanceAmount } from "./rebalanceAmount";
import type {
  Conviction,
  CreateInstrumentInput,
  DateRange,
  GainsResponse,
  HealthResponse,
  HoldingsResponse,
  ImportPreview,
  ImportResult,
  ImportSource,
  Instrument,
  InstrumentLookupResponse,
  PriceHistoryResponse,
  PriceStatusResponse,
  RebalanceRankBy,
  RebalanceResponse,
  RefreshPricesInput,
  RefreshPricesResult,
  ReturnMethod,
  RollbackResult,
  Transaction,
  TransactionType,
  ValueHistoryResponse,
} from "./types";

/**
 * The backend's snapshot heartbeat. Polled quickly while a price refresh runs,
 * slowly otherwise, and re-checked when the window regains focus. Everything
 * else keys off it, so this is the only query that decides when the app as a
 * whole moves to newer data.
 */
export function useDataVersion() {
  return useQuery({
    queryKey: ["data-version"],
    queryFn: () => apiGet<DataVersion>("/api/data-version"),
    refetchInterval: (query) =>
      query.state.data?.prices_refreshing ? 2000 : 15_000,
    // A hidden tab stops polling; returning to it re-checks immediately, which
    // is what the focus refetch is for.
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: 0,
  });
}

/**
 * Define a data query that belongs to one snapshot. The token is part of the
 * cache key, so results from different snapshots can never share an entry, and
 * a version change refetches every panel together.
 *
 * Previous data is kept by default while the new snapshot loads, so a panel
 * shows the older numbers briefly rather than emptying out on every refresh,
 * transaction or restart.
 *
 * A response that names a revision other than the one asked for means the
 * snapshot moved while the request was in flight. Rechecking the version is
 * enough: the token changes and everything refetches together.
 */
interface VersionedQueryOptions<T> {
  enabled?: boolean;
  keepPreviousData?: boolean;
  refetchInterval?: UseQueryOptions<T>["refetchInterval"];
  refetchIntervalInBackground?: boolean;
}

function useVersionedQuery<T>(
  key: readonly unknown[],
  path: string,
  options: VersionedQueryOptions<T> = {},
) {
  const queryClient = useQueryClient();
  const version = useDataVersion();
  const token = version.data ? versionToken(version.data) : null;
  const requestedRevision = version.data?.data_revision ?? null;

  const query = useQuery({
    queryKey: [key[0], token, ...key.slice(1)],
    queryFn: async () => {
      const data = await apiGet<T>(path);
      const served = (data as { data_revision?: string } | null)?.data_revision;
      if (served !== undefined && served !== requestedRevision) {
        void queryClient.invalidateQueries({ queryKey: ["data-version"] });
      }
      return data;
    },
    enabled: token !== null && (options.enabled ?? true),
    placeholderData:
      options.keepPreviousData === false ? undefined : keepPreviousData,
    refetchInterval: options.refetchInterval,
    refetchIntervalInBackground: options.refetchIntervalInBackground,
  });

  const versionUnavailable = token === null && version.isError;
  return {
    ...query,
    error: query.error ?? (versionUnavailable ? version.error : null),
    isError: query.isError || versionUnavailable,
    isPending: query.isPending && !versionUnavailable,
    refetch: versionUnavailable ? version.refetch : query.refetch,
  };
}

export function useInstruments() {
  return useVersionedQuery<Instrument[]>(["instruments"], "/api/instruments");
}

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => apiGet<HealthResponse>("/api/health"),
  });
}

export function useTransactions() {
  return useVersionedQuery<Transaction[]>(
    ["transactions"],
    "/api/transactions",
  );
}

export function lookupInstrument(query: string) {
  const search = new URLSearchParams({ query });
  return apiGet<InstrumentLookupResponse>(`/api/instruments/lookup?${search}`);
}

export function useHoldings(includeWatchlist = false) {
  const search = new URLSearchParams();
  if (includeWatchlist) search.set("include_watchlist", "true");
  const qs = search.toString();
  return useVersionedQuery<HoldingsResponse>(
    ["holdings", includeWatchlist],
    `/api/holdings${qs ? `?${qs}` : ""}`,
  );
}

export interface GainsParams {
  includeClosedPositions?: boolean;
  startDate?: string | null;
  endDate?: string | null;
  method?: ReturnMethod;
}

export function useGains(params: GainsParams = {}) {
  const { includeClosedPositions = false, startDate, endDate, method } = params;
  const search = new URLSearchParams();
  if (includeClosedPositions) search.set("include_closed", "true");
  if (startDate) search.set("start_date", startDate);
  if (endDate) search.set("end_date", endDate);
  if (method) search.set("method", method);
  const qs = search.toString();

  return useVersionedQuery<GainsResponse>(
    [
      "gains",
      includeClosedPositions,
      startDate ?? null,
      endDate ?? null,
      method ?? null,
    ],
    `/api/gains${qs ? `?${qs}` : ""}`,
  );
}

export type { DateRange, ReturnMethod };

export function usePriceStatus() {
  return useVersionedQuery<PriceStatusResponse>(
    ["price-status"],
    "/api/prices/status",
    {
      refetchInterval: (query) => (query.state.data?.refreshing ? 2000 : false),
      refetchIntervalInBackground: true,
    },
  );
}

export function useInstrumentPrices(id: number | null) {
  return useVersionedQuery<PriceHistoryResponse>(
    ["instrument-prices", id],
    `/api/instruments/${id}/prices`,
    { enabled: id !== null, keepPreviousData: false },
  );
}

export function usePortfolioValueHistory() {
  return useVersionedQuery<ValueHistoryResponse>(
    ["portfolio-value-history"],
    "/api/portfolio/value-history",
  );
}

export function useRebalancePlan(
  amount: string | null,
  rankBy: RebalanceRankBy,
) {
  const normalizedAmount = normalizeRebalanceAmount(amount);

  return useVersionedQuery<RebalanceResponse>(
    ["rebalance", normalizedAmount, rankBy],
    `/api/rebalance?amount=${encodeURIComponent(
      normalizedAmount ?? "",
    )}&rank_by=${rankBy}`,
    { enabled: normalizedAmount !== null },
  );
}

export type NewInstrumentInput = CreateInstrumentInput;

export interface UpsertInstrumentResult {
  status: number;
  instrument: Instrument;
}

export interface NewTransactionInput {
  instrument_id: number;
  type: TransactionType;
  trade_date: string;
  quantity: number;
  price?: string;
  dividend_per_share?: string;
  currency?: string;
  fx_rate_to_base?: string;
  brokerage?: string;
  note?: string;
}

export function useUpsertInstrument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (
      input: NewInstrumentInput,
    ): Promise<UpsertInstrumentResult> => {
      const response = await apiSendWithStatus<Instrument>(
        "POST",
        "/api/instruments",
        input,
      );
      return { status: response.status, instrument: response.body };
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export interface ConvictionChange {
  instrument_id: number;
  conviction: Conviction;
}

/** Save one instrument's conviction (Asset Detail). */
export function useUpdateInstrumentConviction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      instrumentId,
      conviction,
    }: {
      instrumentId: number;
      conviction: Conviction;
    }) =>
      apiSend<Instrument>(
        "PUT",
        `/api/instruments/${instrumentId}/conviction`,
        { conviction },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function useDeleteInstrument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (instrumentId: number) =>
      apiSend<void>("DELETE", `/api/instruments/${instrumentId}`, undefined),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

/**
 * Apply several conviction changes at once (Holdings apply-all). The backend
 * validates every id and writes them in one transaction; targets are pool-wide,
 * so holdings must refetch after applying.
 */
export function useUpdateInstrumentConvictions() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (changes: ConvictionChange[]) =>
      apiSend<Instrument[]>("PUT", "/api/instruments/convictions", { changes }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function useCreateTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: NewTransactionInput) =>
      apiSend<Transaction>("POST", "/api/transactions", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function useDeleteTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) =>
      apiSend<void>("DELETE", `/api/transactions/${id}`, undefined),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function useRefreshPrices() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: RefreshPricesInput = { mode: "latest" }) =>
      apiSend<RefreshPricesResult>("POST", "/api/prices/refresh", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function usePreviewImport() {
  return useMutation({
    mutationFn: ({
      source,
      file,
    }: {
      source: ImportSource;
      file: ArrayBuffer;
    }) =>
      apiSendBytes<ImportPreview>(
        "POST",
        `/api/import/${source}/preview`,
        file,
      ),
  });
}

export function useCommitImport() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      source,
      file,
      allowDuplicate,
      exclude,
      mode,
      replaceBatchId,
      convictionKeep,
      convictionToOther,
    }: {
      source: ImportSource;
      file: ArrayBuffer;
      allowDuplicate: boolean;
      exclude: string[];
      mode?: "replace" | "append";
      replaceBatchId?: number;
      convictionKeep?: string[];
      convictionToOther?: string[];
    }) => {
      const params = new URLSearchParams();

      if (allowDuplicate) {
        params.set("allow_duplicate", "true");
      }

      if (exclude.length > 0) {
        params.set("exclude", exclude.join(","));
      }

      if (mode) {
        params.set("mode", mode);
      }

      if (replaceBatchId !== undefined) {
        params.set("replace_batch_id", String(replaceBatchId));
      }

      if (convictionKeep && convictionKeep.length > 0) {
        params.set("conviction_keep", convictionKeep.join(","));
      }

      if (convictionToOther && convictionToOther.length > 0) {
        params.set("conviction_to_other", convictionToOther.join(","));
      }

      const query = params.toString();

      return apiSendBytes<ImportResult>(
        "POST",
        `/api/import/${source}/commit${query ? `?${query}` : ""}`,
        file,
      );
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}

export function useRollbackImport() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (batchId: number) =>
      apiSendBytes<RollbackResult>(
        "POST",
        `/api/import/rollback/${batchId}`,
        new ArrayBuffer(0),
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["data-version"] });
    },
  });
}
