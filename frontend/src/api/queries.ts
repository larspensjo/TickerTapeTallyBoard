import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiSend, apiSendBytes } from "./client";
import type {
  Holding,
  ImportPreview,
  ImportResult,
  Instrument,
  InstrumentType,
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
    },
  });
}

export function usePreviewImport() {
  return useMutation({
    mutationFn: (file: ArrayBuffer) =>
      apiSendBytes<ImportPreview>(
        "POST",
        "/api/import/sharesight/preview",
        file,
      ),
  });
}

export function useCommitImport() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      file,
      allowDuplicate,
    }: {
      file: ArrayBuffer;
      allowDuplicate: boolean;
    }) =>
      apiSendBytes<ImportResult>(
        "POST",
        `/api/import/sharesight/commit${allowDuplicate ? "?allow_duplicate=true" : ""}`,
        file,
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}

export function useRollbackImport() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (batchId: number) =>
      apiSendBytes<RollbackResult>(
        "POST",
        `/api/import/sharesight/rollback/${batchId}`,
        new ArrayBuffer(0),
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["instruments"] });
      void queryClient.invalidateQueries({ queryKey: ["transactions"] });
      void queryClient.invalidateQueries({ queryKey: ["holdings"] });
    },
  });
}
