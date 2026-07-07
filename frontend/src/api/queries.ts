import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { apiGet, apiSend, apiSendBytes } from "./client";
import { normalizeRebalanceAmount } from "./rebalanceAmount";
import type {
  Conviction,
  DateRange,
  GainsResponse,
  HealthResponse,
  Holding,
  ImportPreview,
  ImportResult,
  ImportSource,
  Instrument,
  InstrumentType,
  PriceHistoryResponse,
  PriceStatusResponse,
  RebalanceResponse,
  RefreshPricesInput,
  RefreshPricesResult,
  ReturnMethod,
  RollbackResult,
  Transaction,
  TransactionType,
  ValueHistoryResponse,
} from "./types";

export function useInstruments() {
  return useQuery({
    queryKey: ["instruments"],
    queryFn: () => apiGet<Instrument[]>("/api/instruments"),
  });
}

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => apiGet<HealthResponse>("/api/health"),
  });
}

export function useTransactions() {
  return useQuery({
    queryKey: ["transactions"],
    queryFn: () => apiGet<Transaction[]>("/api/transactions"),
  });
}

export function useHoldings() {
  return useQuery({
    queryKey: ["holdings"],
    queryFn: () => apiGet<Holding[]>("/api/holdings"),
  });
}

export interface GainsParams {
  includeClosedPositions?: boolean;
  startDate?: string | null;
  endDate?: string | null;
  method?: ReturnMethod;
}

export function useGains(params: GainsParams = {}) {
  const { includeClosedPositions = false, startDate, endDate, method } = params;
  return useQuery({
    queryKey: [
      "gains",
      includeClosedPositions,
      startDate ?? null,
      endDate ?? null,
      method ?? null,
    ],
    queryFn: () => {
      const search = new URLSearchParams();
      if (includeClosedPositions) search.set("include_closed", "true");
      if (startDate) search.set("start_date", startDate);
      if (endDate) search.set("end_date", endDate);
      if (method) search.set("method", method);
      const qs = search.toString();
      return apiGet<GainsResponse>(`/api/gains${qs ? `?${qs}` : ""}`);
    },
    placeholderData: keepPreviousData,
  });
}

export type { DateRange, ReturnMethod };

export function usePriceStatus() {
  return useQuery({
    queryKey: ["price-status"],
    queryFn: () => apiGet<PriceStatusResponse>("/api/prices/status"),
    refetchInterval: (query) => (query.state.data?.refreshing ? 2000 : false),
    refetchIntervalInBackground: true,
  });
}

export function useInstrumentPrices(id: number | null) {
  return useQuery({
    queryKey: ["instrument-prices", id],
    queryFn: () =>
      apiGet<PriceHistoryResponse>(`/api/instruments/${id}/prices`),
    enabled: id !== null,
  });
}

export function usePortfolioValueHistory() {
  return useQuery({
    queryKey: ["portfolio-value-history"],
    queryFn: () => apiGet<ValueHistoryResponse>("/api/portfolio/value-history"),
  });
}

export function useRebalancePlan(amount: string | null) {
  const normalizedAmount = normalizeRebalanceAmount(amount);

  return useQuery({
    queryKey: ["rebalance", normalizedAmount],
    queryFn: () =>
      apiGet<RebalanceResponse>(
        `/api/rebalance?amount=${encodeURIComponent(normalizedAmount ?? "")}`,
      ),
    enabled: normalizedAmount !== null,
    placeholderData: keepPreviousData,
  });
}

export interface NewInstrumentInput {
  symbol: string;
  exchange: string;
  name: string;
  type: InstrumentType;
  currency: string;
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

function invalidatePortfolioData(
  queryClient: ReturnType<typeof useQueryClient>,
): void {
  void queryClient.invalidateQueries({ queryKey: ["transactions"] });
  void queryClient.invalidateQueries({ queryKey: ["holdings"] });
  void queryClient.invalidateQueries({ queryKey: ["gains"] });
  void queryClient.invalidateQueries({ queryKey: ["price-status"] });
  void queryClient.invalidateQueries({ queryKey: ["portfolio-value-history"] });
}

export function useUpsertInstrument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: NewInstrumentInput) =>
      apiSend<Instrument>("POST", "/api/instruments", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
    },
  });
}

export interface ConvictionChange {
  instrument_id: number;
  conviction: Conviction;
}

/**
 * Save one instrument's conviction (Asset Detail). Conviction is portfolio
 * metadata, so only instruments and holdings are invalidated — not gains,
 * price status, or value history.
 */
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
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
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
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}

export function useCreateTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: NewTransactionInput) =>
      apiSend<Transaction>("POST", "/api/transactions", input),
    onSuccess: () => {
      invalidatePortfolioData(queryClient);
    },
  });
}

export function useDeleteTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) =>
      apiSend<void>("DELETE", `/api/transactions/${id}`, undefined),
    onSuccess: () => {
      invalidatePortfolioData(queryClient);
    },
  });
}

export function useRefreshPrices() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: RefreshPricesInput = { mode: "latest" }) =>
      apiSend<RefreshPricesResult>("POST", "/api/prices/refresh", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
      void queryClient.invalidateQueries({ queryKey: ["gains"] });
      void queryClient.invalidateQueries({ queryKey: ["price-status"] });
      void queryClient.invalidateQueries({ queryKey: ["instrument-prices"] });
      void queryClient.invalidateQueries({
        queryKey: ["portfolio-value-history"],
      });
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
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      invalidatePortfolioData(queryClient);
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
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      invalidatePortfolioData(queryClient);
    },
  });
}
