import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiSend, apiSendBytes } from "./client";
import type {
  DateRange,
  GainsResponse,
  Holding,
  ImportPreview,
  ImportResult,
  ImportSource,
  Instrument,
  InstrumentType,
  PriceStatusResponse,
  RefreshPricesInput,
  RefreshPricesResult,
  RollbackResult,
  Transaction,
  TransactionType,
} from "./types";

export function useInstruments() {
  return useQuery({
    queryKey: ["instruments"],
    queryFn: () => apiGet<Instrument[]>("/api/instruments"),
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
}

export function useGains(params: GainsParams = {}) {
  const { includeClosedPositions = false, startDate, endDate } = params;
  return useQuery({
    queryKey: [
      "gains",
      includeClosedPositions,
      startDate ?? null,
      endDate ?? null,
    ],
    queryFn: () => {
      const search = new URLSearchParams();
      if (includeClosedPositions) search.set("include_closed", "true");
      if (startDate) search.set("start_date", startDate);
      if (endDate) search.set("end_date", endDate);
      const qs = search.toString();
      return apiGet<GainsResponse>(`/api/gains${qs ? `?${qs}` : ""}`);
    },
  });
}

export type { DateRange };

export function usePriceStatus() {
  return useQuery({
    queryKey: ["price-status"],
    queryFn: () => apiGet<PriceStatusResponse>("/api/prices/status"),
    refetchInterval: (query) => (query.state.data?.refreshing ? 2000 : false),
    refetchIntervalInBackground: true,
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
  currency?: string;
  fx_rate_to_base?: string;
  brokerage?: string;
  note?: string;
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

export function useCreateTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: NewTransactionInput) =>
      apiSend<Transaction>("POST", "/api/transactions", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
      void queryClient.invalidateQueries({ queryKey: ["gains"] });
    },
  });
}

export function useDeleteTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) =>
      apiSend<void>("DELETE", `/api/transactions/${id}`, undefined),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
      void queryClient.invalidateQueries({ queryKey: ["gains"] });
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
    }: {
      source: ImportSource;
      file: ArrayBuffer;
      allowDuplicate: boolean;
      exclude: string[];
    }) => {
      const params = new URLSearchParams();

      if (allowDuplicate) {
        params.set("allow_duplicate", "true");
      }

      if (exclude.length > 0) {
        params.set("exclude", exclude.join(","));
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
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
      void queryClient.invalidateQueries({ queryKey: ["gains"] });
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
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
      void queryClient.invalidateQueries({ queryKey: ["gains"] });
    },
  });
}
