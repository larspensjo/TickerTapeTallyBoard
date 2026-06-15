import type { ApiErrorBody } from "./types";

export class ApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

async function parseJson(text: string): Promise<unknown> {
  if (!text) {
    return null;
  }

  try {
    return JSON.parse(text) as unknown;
  } catch {
    return null;
  }
}

async function parse<T>(response: Response): Promise<T> {
  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  const body = await parseJson(text);

  if (!response.ok) {
    const error = (body as ApiErrorBody | null)?.error;
    throw new ApiError(
      error?.code ?? "unknown",
      error?.message ?? `Request failed: ${response.status}`,
    );
  }

  return body as T;
}

export async function apiGet<T>(path: string): Promise<T> {
  return parse<T>(await fetch(path));
}

export async function apiSend<T>(
  method: string,
  path: string,
  body: unknown,
): Promise<T> {
  return parse<T>(
    await fetch(path, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
  );
}

export async function apiSendBytes<T>(
  method: string,
  path: string,
  body: ArrayBuffer,
): Promise<T> {
  return parse<T>(
    await fetch(path, {
      method,
      headers: { "content-type": "text/csv" },
      body,
    }),
  );
}
