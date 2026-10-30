// One door to the Trade API (token lists, charts, holders, trades, perps, swap quotes and unsigned
// builds). In the app the Rust trade_api command makes the call (most paths send no CORS
// headers); in browser dev the Vite proxy at /trade-api does. Nothing here signs or submits.

import { isTauri } from "../lib/bridge";

export class TradeApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "TradeApiError";
  }
}

export interface Reply<T> {
  status: number;
  body: T;
}

/** Raw call: any status comes back, network failures throw. */
export async function tradeCall<T>(method: "GET" | "POST", path: string, body?: unknown, auth?: string): Promise<Reply<T>> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<Reply<T>>("trade_api", { method, path, body: body ?? null, auth: auth ?? null });
  }
  const r = await fetch(`/trade-api${path}`, {
    method,
    headers: { ...(body !== undefined ? { "content-type": "application/json" } : {}), ...(auth ? { authorization: `Bearer ${auth}` } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let parsed: unknown = null;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    parsed = null;
  }
  return { status: r.status, body: parsed as T };
}

/** The error message an API answer carries, in plain words. */
export function errorOf(status: number, body: unknown): TradeApiError {
  const b = body as { error?: string | { code?: string; message?: string } } | null;
  const e = typeof b?.error === "object" && b.error ? b.error : { message: typeof b?.error === "string" ? b.error : undefined };
  const message =
    status === 429 ? "Too many requests. Try again in a moment." : status === 204 ? "No route for this pair right now." : e.message || `Trade service error (${status})`;
  return new TradeApiError(status, e.code ?? "http_error", message);
}

export async function tradeGet<T>(path: string): Promise<T> {
  const r = await tradeCall<T>("GET", path);
  if (r.status < 200 || r.status >= 300) throw errorOf(r.status, r.body);
  return r.body;
}

export async function tradePost<T>(path: string, body: unknown, auth?: string): Promise<T> {
  const r = await tradeCall<T>("POST", path, body, auth);
  if (r.status < 200 || r.status >= 300) throw errorOf(r.status, r.body);
  return r.body;
}
