/**
 * Minimal typed fetch client for the Zhi-Mart backend (`/api/v1`).
 *
 * - Always sends `credentials: 'include'` so the HttpOnly refresh cookie travels
 *   with `/auth/*` requests, plus `X-Requested-With: XMLHttpRequest` (CSRF guard).
 * - Normalises every failure into an `ApiError`
 *   (`{ error: { code, message, attemptsRemaining?, retryAfter? } }`).
 */

const RAW_BASE = (import.meta.env.VITE_API_BASE_URL ?? '').trim().replace(/\/+$/, '');

export const API_BASE_URL = `${RAW_BASE}/api/v1`;

// Longer than the server's 15 s request timeout, so the server's answer (even a
// 503) always arrives before the client gives up on a request it may have committed.
const DEFAULT_TIMEOUT_MS = 20_000;

// ---------------------------------------------------------------------------
// Contract types (see backend/API.md)
// ---------------------------------------------------------------------------

export interface AdminIdentity {
  id: string;
  email: string;
  displayName: string;
}

export interface AuthSession {
  accessToken: string;
  tokenType: 'Bearer' | string;
  /** Access-token lifetime in seconds. */
  expiresIn: number;
  /** ISO-8601 absolute (30-day) session expiry. */
  sessionExpiresAt: string;
  admin: AdminIdentity;
}

export interface LoginChallenge {
  challengeId: string;
  maskedEmail: string;
  /** OTP lifetime in seconds. */
  expiresIn: number;
  /** Seconds until "resend code" is allowed. */
  resendAvailableIn: number;
}

export interface OtpResendResult {
  expiresIn: number;
  resendAvailableIn: number;
}

export interface MeResponse {
  admin: AdminIdentity;
  sessionExpiresAt: string;
}

export type ApiErrorCode =
  | 'validation_error'
  | 'invalid_credentials'
  | 'otp_invalid'
  | 'otp_challenge_expired'
  | 'unauthorized'
  | 'session_expired'
  | 'forbidden'
  | 'too_many_requests'
  | 'account_locked'
  | 'service_unavailable'
  | 'not_found'
  | 'out_of_stock'
  | 'product_unavailable'
  | 'version_conflict'
  | 'invalid_transition'
  | 'duplicate_id'
  | 'idempotency_conflict'
  | 'payload_too_large'
  | 'order_already_paid'
  | 'order_expired'
  | 'order_not_payable'
  | 'payment_verification_failed'
  | 'payment_pending'
  | 'payment_not_completed'
  | 'payment_gateway_error'
  | 'online_payments_disabled'
  | 'network_error'
  | 'timeout'
  | 'unknown_error'
  | (string & {});

/** One order line that exceeds available stock (`out_of_stock` errors). */
export interface StockShortage {
  productId: string;
  requested: number;
  available: number;
}

export class ApiError extends Error {
  readonly status: number;
  readonly code: ApiErrorCode;
  readonly attemptsRemaining?: number;
  /** Seconds the client should wait before retrying. */
  readonly retryAfter?: number;
  /** Present on `out_of_stock`. */
  readonly details?: StockShortage[];

  constructor(
    status: number,
    code: ApiErrorCode,
    message: string,
    extra: { attemptsRemaining?: number; retryAfter?: number; details?: StockShortage[] } = {}
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.attemptsRemaining = extra.attemptsRemaining;
    this.retryAfter = extra.retryAfter;
    this.details = extra.details;
  }
}

export const isApiError = (err: unknown): err is ApiError => err instanceof ApiError;

// ---------------------------------------------------------------------------
// Request helper
// ---------------------------------------------------------------------------

export interface ApiRequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  /** JSON-serialisable body. */
  body?: unknown;
  /** Bearer token to attach as `Authorization`. */
  accessToken?: string | null;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  timeoutMs?: number;
}

function parseRetryAfterHeader(value: string | null): number | undefined {
  if (!value) return undefined;
  const secs = Number(value);
  if (Number.isFinite(secs) && secs >= 0) return Math.ceil(secs);
  const date = Date.parse(value);
  if (!Number.isNaN(date)) return Math.max(0, Math.ceil((date - Date.now()) / 1000));
  return undefined;
}

function defaultMessageFor(status: number): string {
  if (status === 401) return 'Your session is not authorized.';
  if (status === 403) return 'This request was blocked.';
  if (status === 429) return 'Too many requests. Please wait and try again.';
  if (status >= 500) return 'The server is temporarily unavailable. Please try again.';
  return 'Something went wrong. Please try again.';
}

async function toApiError(res: Response): Promise<ApiError> {
  let payload: unknown = null;
  try {
    payload = await res.json();
  } catch {
    // Non-JSON error body (e.g. proxy error page) — fall through to defaults.
  }

  const err =
    payload && typeof payload === 'object' && 'error' in payload
      ? ((payload as { error: Record<string, unknown> }).error ?? {})
      : {};

  const code =
    typeof err.code === 'string'
      ? err.code
      : res.status === 429
        ? 'too_many_requests'
        : res.status === 502 || res.status === 503 || res.status === 504
          ? 'service_unavailable'
          : 'unknown_error';
  const message = typeof err.message === 'string' ? err.message : defaultMessageFor(res.status);
  const attemptsRemaining =
    typeof err.attemptsRemaining === 'number' ? err.attemptsRemaining : undefined;
  const retryAfter =
    typeof err.retryAfter === 'number'
      ? Math.max(0, Math.ceil(err.retryAfter))
      : parseRetryAfterHeader(res.headers.get('Retry-After'));

  const details = Array.isArray(err.details) ? (err.details as StockShortage[]) : undefined;

  return new ApiError(res.status, code, message, { attemptsRemaining, retryAfter, details });
}

export async function apiRequest<T = unknown>(
  path: string,
  { method = 'GET', body, accessToken, headers, signal, timeoutMs = DEFAULT_TIMEOUT_MS }: ApiRequestOptions = {}
): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const onExternalAbort = () => controller.abort();
  signal?.addEventListener('abort', onExternalAbort);

  const finalHeaders: Record<string, string> = {
    Accept: 'application/json',
    'X-Requested-With': 'XMLHttpRequest',
    ...headers,
  };
  if (body !== undefined) finalHeaders['Content-Type'] = 'application/json';
  if (accessToken) finalHeaders.Authorization = `Bearer ${accessToken}`;

  let res: Response;
  try {
    res = await fetch(`${API_BASE_URL}${path.startsWith('/') ? path : `/${path}`}`, {
      method,
      headers: finalHeaders,
      body: body !== undefined ? JSON.stringify(body) : undefined,
      credentials: 'include',
      cache: 'no-store',
      signal: controller.signal,
    });
  } catch (cause) {
    if (signal?.aborted) throw cause;
    const timedOut = controller.signal.aborted;
    throw new ApiError(
      0,
      timedOut ? 'timeout' : 'network_error',
      timedOut
        ? 'The server took too long to respond. Please try again.'
        : "Can't reach the server. Check your connection and try again."
    );
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', onExternalAbort);
  }

  if (!res.ok) throw await toApiError(res);

  if (res.status === 204 || res.status === 205) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new ApiError(res.status, 'unknown_error', 'Received an unexpected response from the server.');
  }
}

// ---------------------------------------------------------------------------
// Auth endpoints
// ---------------------------------------------------------------------------

export const authApi = {
  login: (email: string, password: string) =>
    apiRequest<LoginChallenge>('/auth/login', { method: 'POST', body: { email, password } }),

  verifyOtp: (challengeId: string, code: string) =>
    apiRequest<AuthSession>('/auth/otp/verify', { method: 'POST', body: { challengeId, code } }),

  resendOtp: (challengeId: string) =>
    apiRequest<OtpResendResult>('/auth/otp/resend', { method: 'POST', body: { challengeId } }),

  refresh: () => apiRequest<AuthSession>('/auth/refresh', { method: 'POST' }),

  logout: (accessToken?: string | null) =>
    apiRequest<void>('/auth/logout', { method: 'POST', accessToken }),

  me: (accessToken: string) => apiRequest<MeResponse>('/auth/me', { accessToken }),
};
