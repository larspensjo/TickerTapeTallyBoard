import type { OnChangeFn, SortingState } from "@tanstack/react-table";
import { useCallback, useMemo, useState } from "react";

// Shared client-side persistence for view settings (sort order, view/chart mode,
// display toggles). Per the "View settings are persisted client-side" decision,
// loading always tolerates a missing or malformed value by falling back to the
// default, and unknown/retired option values are rejected on load.

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/**
 * Read a JSON-serialized setting, returning `fallback` when the key is absent,
 * unparseable, or fails `isValid`.
 */
export function loadSetting<T>(
  key: string,
  isValid: (value: unknown) => value is T,
  fallback: T,
): T {
  const saved = storage()?.getItem(key);
  if (saved == null) return fallback;

  try {
    const parsed: unknown = JSON.parse(saved);
    return isValid(parsed) ? parsed : fallback;
  } catch {
    return fallback;
  }
}

/**
 * Persist a setting as JSON. Storage failures are ignored so the setting still
 * applies for the current session, and values that fail `isValid` are not
 * written.
 */
export function saveSetting<T>(
  key: string,
  value: T,
  isValid: (value: unknown) => value is T,
): void {
  if (!isValid(value)) return;

  try {
    storage()?.setItem(key, JSON.stringify(value));
  } catch {
    // Ignore storage failures; the setting still applies for this session.
  }
}

/** Validator for a value drawn from a fixed set of string options. */
export function isOneOf<T extends string>(
  allowed: readonly T[],
): (value: unknown) => value is T {
  const set = new Set<string>(allowed);
  return (value): value is T => typeof value === "string" && set.has(value);
}

export function isBoolean(value: unknown): value is boolean {
  return typeof value === "boolean";
}

/**
 * Validator for a TanStack `SortingState` restricted to `sortableIds`. Rejects
 * duplicate columns and any id not in the set, so a stale stored value pointing
 * at a retired column falls back to the default.
 */
export function isSortingStateFor(
  sortableIds: ReadonlySet<string>,
): (value: unknown) => value is SortingState {
  return (value): value is SortingState => {
    if (!Array.isArray(value)) return false;

    const seen = new Set<string>();
    return value.every((sort) => {
      if (typeof sort !== "object" || sort === null) return false;

      const candidate = sort as Partial<SortingState[number]>;
      if (
        typeof candidate.id !== "string" ||
        typeof candidate.desc !== "boolean" ||
        !sortableIds.has(candidate.id) ||
        seen.has(candidate.id)
      ) {
        return false;
      }

      seen.add(candidate.id);
      return true;
    });
  };
}

/**
 * Persisted view-setting state. Behaves like `useState` with a single value,
 * loading the initial value from storage and writing every update back.
 */
export function usePersistentSetting<T>(
  key: string,
  isValid: (value: unknown) => value is T,
  fallback: T,
): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() =>
    loadSetting(key, isValid, fallback),
  );
  const set = useCallback(
    (next: T) => {
      setValue(next);
      saveSetting(key, next, isValid);
    },
    [key, isValid],
  );
  return [value, set];
}

/**
 * Persisted TanStack table sorting. Returns the current sorting and an
 * `onSortingChange` handler that writes each change back to storage.
 */
export function usePersistentSorting(
  key: string,
  sortableIds: ReadonlySet<string>,
  defaultSorting: SortingState,
): [SortingState, OnChangeFn<SortingState>] {
  const isValid = useMemo(() => isSortingStateFor(sortableIds), [sortableIds]);
  const [sorting, setSorting] = useState<SortingState>(() =>
    loadSetting(key, isValid, defaultSorting),
  );
  const onSortingChange = useCallback<OnChangeFn<SortingState>>(
    (updater) => {
      setSorting((current) => {
        const next = typeof updater === "function" ? updater(current) : updater;
        saveSetting(key, next, isValid);
        return next;
      });
    },
    [key, isValid],
  );
  return [sorting, onSortingChange];
}
