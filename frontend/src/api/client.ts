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

export interface ApiResponse<T> {
  status: number;
  body: T;
}

async function parseBody<T>(response: Response): Promise<T> {
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

async function parse<T>(response: Response): Promise<T> {
  return (await parseBody<T>(response)) as T;
}

async function parseWithStatus<T>(response: Response): Promise<ApiResponse<T>> {
  return {
    status: response.status,
    body: await parseBody<T>(response),
  };
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

export async function apiSendWithStatus<T>(
  method: string,
  path: string,
  body: unknown,
): Promise<ApiResponse<T>> {
  return parseWithStatus<T>(
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
