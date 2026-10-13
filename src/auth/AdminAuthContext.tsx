import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  ApiError,
  ApiRequestOptions,
  AdminIdentity,
  AuthSession,
  LoginChallenge,
  OtpResendResult,
  apiRequest,
  authApi,
  isApiError,
} from '../lib/api';

/**
 * Admin authentication state.
 *
 * Security model:
 * - The access token lives ONLY in a ref inside this provider (memory). It is never
 *   written to localStorage/sessionStorage/cookies and is never broadcast to other tabs.
 * - The refresh token is an HttpOnly cookie that JS cannot read; `/auth/refresh`
 *   rotates it on every call and re-use of an old one revokes the session, so every
 *   refresh is single-flight within a tab and serialised across tabs with the
 *   Web Locks API.
 * - The session hard-stops at `sessionExpiresAt` (30 days after OTP verification).
 * - Logout is retried; until the server confirms it, a non-secret "signed out" marker
 *   (`zm_admin_signed_out` in localStorage) blocks silent session restore so a failed
 *   logout can't silently sign the admin back in on reload (shared-computer risk).
 *   Only an explicit password + OTP login clears it.
 */

export type AdminAuthStatus = 'loading' | 'authenticated' | 'unauthenticated';

export const SESSION_ENDED_MESSAGE = 'Your 30-day admin session has ended. Please sign in again.';
const SESSION_REVOKED_MESSAGE = 'Your admin session has expired. Please sign in again.';

const REFRESH_LOCK_NAME = 'zm-admin-refresh';
const BROADCAST_CHANNEL_NAME = 'zm-admin-auth';
/** Refresh this long before the access token expires. */
const REFRESH_LEAD_MS = 60_000;
/** Retry delay after a transient (network/5xx) refresh failure. */
const TRANSIENT_RETRY_MS = 15_000;
/** Max delay accepted by setTimeout (~24.8 days); longer delays fire immediately. */
const MAX_TIMEOUT_MS = 2_147_483_647;
/** Legacy demo flag from the pre-backend fake login; removed on startup. */
const LEGACY_ADMIN_FLAG_KEY = 'zhi_mart_admin';
/** Boolean marker (no secret): set while a logout is not yet confirmed by the server. */
const SIGNED_OUT_MARKER_KEY = 'zm_admin_signed_out';
/** Backoff between logout attempts (3 attempts total). */
const LOGOUT_RETRY_DELAYS_MS = [1_000, 3_000];

export const LOGOUT_INCOMPLETE_MESSAGE =
  "We couldn't reach the server to end your session; it will expire automatically. Close the browser on shared computers.";

function hasSignedOutMarker(): boolean {
  try {
    return localStorage.getItem(SIGNED_OUT_MARKER_KEY) === '1';
  } catch {
    return false;
  }
}

/** Returns whether the marker could be persisted. */
function setSignedOutMarker(on: boolean): boolean {
  try {
    if (on) localStorage.setItem(SIGNED_OUT_MARKER_KEY, '1');
    else localStorage.removeItem(SIGNED_OUT_MARKER_KEY);
    return true;
  } catch {
    // Storage unavailable — fall back to in-memory behaviour only.
    return false;
  }
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Transient failures worth retrying for logout (network, timeout, 429, 5xx). */
function isRetryableLogoutError(err: unknown): boolean {
  if (!isApiError(err)) return true;
  return err.status === 0 || err.status === 429 || err.status >= 500;
}

type BroadcastMessage =
  | { type: 'login' }
  | { type: 'logout'; reason?: 'session_ended' };

export interface AdminAuthContextValue {
  status: AdminAuthStatus;
  admin: AdminIdentity | null;
  sessionExpiresAt: Date | null;
  /** Why the user was signed out (e.g. 30-day limit), for display on the login screen. */
  sessionEndedReason: string | null;
  clearSessionEndedReason: () => void;
  login: (email: string, password: string) => Promise<LoginChallenge>;
  verifyOtp: (challengeId: string, code: string) => Promise<AuthSession>;
  resendOtp: (challengeId: string) => Promise<OtpResendResult>;
  logout: () => Promise<void>;
  /** Calls a protected endpoint with the Bearer token; refreshes once on 401 `unauthorized`. */
  authFetch: <T = unknown>(path: string, init?: Omit<ApiRequestOptions, 'accessToken'>) => Promise<T>;
}

const AdminAuthContext = createContext<AdminAuthContextValue | null>(null);

/** Run `fn` while holding a cross-tab exclusive lock when the Web Locks API is available. */
async function withRefreshLock<T>(fn: () => Promise<T>): Promise<T> {
  const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined;
  if (locks?.request) {
    return locks.request(REFRESH_LOCK_NAME, { mode: 'exclusive' }, fn) as Promise<T>;
  }
  return fn();
}

/** Definitive "this session is gone" responses from /auth/refresh. */
function isAuthRejection(err: unknown): err is ApiError {
  return isApiError(err) && (err.status === 401 || err.status === 403);
}

export const AdminAuthProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [status, setStatus] = useState<AdminAuthStatus>('loading');
  const [admin, setAdmin] = useState<AdminIdentity | null>(null);
  const [sessionExpiresAt, setSessionExpiresAt] = useState<Date | null>(null);
  const [sessionEndedReason, setSessionEndedReason] = useState<string | null>(null);

  // In-memory only.
  const accessTokenRef = useRef<string | null>(null);
  const accessExpiresAtRef = useRef<number>(0);
  const sessionExpiresAtRef = useRef<number>(0);
  const statusRef = useRef<AdminAuthStatus>('loading');

  const refreshPromiseRef = useRef<Promise<AuthSession | null> | null>(null);
  /** In-flight server-side logout; a new OTP verification waits for it to settle. */
  const pendingLogoutRef = useRef<Promise<boolean> | null>(null);
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sessionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const channelRef = useRef<BroadcastChannel | null>(null);
  /** Bumped on every local sign-out so stale in-flight refreshes can't resurrect a session. */
  const epochRef = useRef(0);

  const setStatusBoth = useCallback((next: AdminAuthStatus) => {
    statusRef.current = next;
    setStatus(next);
  }, []);

  const broadcast = useCallback((msg: BroadcastMessage) => {
    try {
      channelRef.current?.postMessage(msg);
    } catch {
      // Channel closed / unsupported — cross-tab sync is best-effort.
    }
  }, []);

  const clearTimers = useCallback(() => {
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
    if (sessionTimerRef.current) clearTimeout(sessionTimerRef.current);
    refreshTimerRef.current = null;
    sessionTimerRef.current = null;
  }, []);

  /** Drop all in-memory auth state (no network). */
  const clearLocal = useCallback(
    (reason: string | null) => {
      epochRef.current += 1;
      clearTimers();
      accessTokenRef.current = null;
      accessExpiresAtRef.current = 0;
      sessionExpiresAtRef.current = 0;
      setAdmin(null);
      setSessionExpiresAt(null);
      setSessionEndedReason(reason);
      setStatusBoth('unauthenticated');
    },
    [clearTimers, setStatusBoth]
  );

  // Late-bound references so timers/handlers always call the latest implementation.
  const refreshRef = useRef<() => Promise<AuthSession | null>>(async () => null);
  const endSessionRef = useRef<() => void>(() => {});

  const scheduleRefresh = useCallback((delayMs: number) => {
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
    refreshTimerRef.current = setTimeout(
      () => {
        refreshTimerRef.current = null;
        refreshRef.current().catch(() => {
          /* handled inside refresh */
        });
      },
      Math.min(Math.max(delayMs, 1_000), MAX_TIMEOUT_MS)
    );
  }, []);

  const scheduleSessionEnd = useCallback((expiresAtMs: number) => {
    if (sessionTimerRef.current) clearTimeout(sessionTimerRef.current);
    const arm = () => {
      const remaining = expiresAtMs - Date.now();
      if (remaining <= 0) {
        sessionTimerRef.current = null;
        endSessionRef.current();
        return;
      }
      // setTimeout overflows above ~24.8 days, so re-arm in chunks.
      sessionTimerRef.current = setTimeout(arm, Math.min(remaining, MAX_TIMEOUT_MS));
    };
    arm();
  }, []);

  const applySession = useCallback(
    (session: AuthSession) => {
      const now = Date.now();
      const sessionEnd = Date.parse(session.sessionExpiresAt);
      const accessLifetimeMs = Math.max(0, session.expiresIn) * 1000;

      accessTokenRef.current = session.accessToken;
      accessExpiresAtRef.current = now + accessLifetimeMs;
      sessionExpiresAtRef.current = Number.isNaN(sessionEnd) ? 0 : sessionEnd;

      setAdmin(session.admin);
      setSessionExpiresAt(Number.isNaN(sessionEnd) ? null : new Date(sessionEnd));
      setSessionEndedReason(null);
      setStatusBoth('authenticated');

      // Refresh ~60s before expiry (or halfway through for very short tokens).
      const lead = accessLifetimeMs > 2 * REFRESH_LEAD_MS ? REFRESH_LEAD_MS : accessLifetimeMs / 2;
      let refreshAt = now + accessLifetimeMs - lead;
      // No point refreshing past the hard session end — the session timer handles that.
      if (sessionExpiresAtRef.current && refreshAt >= sessionExpiresAtRef.current) {
        if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      } else {
        scheduleRefresh(refreshAt - now);
      }
      if (sessionExpiresAtRef.current) scheduleSessionEnd(sessionExpiresAtRef.current);
    },
    [scheduleRefresh, scheduleSessionEnd, setStatusBoth]
  );

  /**
   * Single-flight refresh. Resolves with the new session, or `null` when the server
   * rejected the refresh cookie (state is cleared). Throws `ApiError` on transient failures.
   */
  const refreshSession = useCallback((): Promise<AuthSession | null> => {
    if (refreshPromiseRef.current) return refreshPromiseRef.current;

    // Signed out on this browser and the server hasn't confirmed it: never restore silently.
    if (hasSignedOutMarker()) {
      if (statusRef.current === 'authenticated') clearLocal(null);
      else if (statusRef.current === 'loading') setStatusBoth('unauthenticated');
      return Promise.resolve(null);
    }

    const epoch = epochRef.current;
    const wasAuthenticated = statusRef.current === 'authenticated';

    const promise = withRefreshLock(async () => {
      // Signed out while waiting for the lock — don't touch the cookie.
      if (epoch !== epochRef.current) return null;
      if (hasSignedOutMarker()) {
        if (statusRef.current !== 'unauthenticated') clearLocal(null);
        return null;
      }
      try {
        const session = await authApi.refresh();
        if (epoch !== epochRef.current) return null; // signed out meanwhile
        applySession(session);
        return session;
      } catch (err) {
        if (epoch !== epochRef.current) return null;
        if (isAuthRejection(err)) {
          if (err.code === 'forbidden') {
            console.warn('[admin-auth] refresh rejected by CSRF/origin check', err);
          }
          const pastHardStop =
            sessionExpiresAtRef.current > 0 && Date.now() >= sessionExpiresAtRef.current - 5_000;
          clearLocal(
            !wasAuthenticated ? null : pastHardStop ? SESSION_ENDED_MESSAGE : SESSION_REVOKED_MESSAGE
          );
          return null;
        }
        // Transient (network / 5xx / 429): keep the session and retry shortly. The refresh
        // cookie is still valid server-side; the 30-day hard stop still applies.
        if (wasAuthenticated && statusRef.current === 'authenticated') {
          const retryMs =
            isApiError(err) && err.retryAfter ? err.retryAfter * 1000 : TRANSIENT_RETRY_MS;
          scheduleRefresh(retryMs);
        }
        throw err;
      }
    }).finally(() => {
      refreshPromiseRef.current = null;
    });

    refreshPromiseRef.current = promise;
    return promise;
  }, [applySession, clearLocal, scheduleRefresh, setStatusBoth]);

  refreshRef.current = refreshSession;

  /**
   * Revoke the server session (cookie-authenticated), retrying transient failures with
   * backoff. The signed-out marker is set first and only removed once the server confirms,
   * so closing the tab mid-logout or a failed logout still blocks silent restore.
   * Resolves `true` when the server confirmed the logout.
   */
  const revokeServerSession = useCallback((token: string | null): Promise<boolean> => {
    const markerPersisted = setSignedOutMarker(true);
    const run = async (): Promise<boolean> => {
      for (let attempt = 0; ; attempt++) {
        try {
          // Serialised (with refreshes and OTP verification) so we revoke using the latest
          // cookie and never revoke a session created by a newer explicit login.
          const superseded = await withRefreshLock(async () => {
            if (markerPersisted && !hasSignedOutMarker()) return true; // someone logged in since
            await authApi.logout(token);
            return false;
          });
          if (!superseded) setSignedOutMarker(false);
          return true;
        } catch (err) {
          if (attempt >= LOGOUT_RETRY_DELAYS_MS.length || !isRetryableLogoutError(err)) {
            return false;
          }
          const retryAfterMs = isApiError(err) && err.retryAfter ? err.retryAfter * 1000 : 0;
          await sleep(Math.min(Math.max(LOGOUT_RETRY_DELAYS_MS[attempt], retryAfterMs), 10_000));
        }
      }
    };
    const promise = run();
    pendingLogoutRef.current = promise;
    promise.finally(() => {
      if (pendingLogoutRef.current === promise) pendingLogoutRef.current = null;
    });
    return promise;
  }, []);

  endSessionRef.current = () => {
    const token = accessTokenRef.current;
    clearLocal(SESSION_ENDED_MESSAGE);
    broadcast({ type: 'logout', reason: 'session_ended' });
    // Let the server clear the cookie (guards against client/server clock skew).
    void revokeServerSession(token);
  };

  // ---- Startup: legacy cleanup, silent restore, cross-tab sync, wake-up checks ----
  useEffect(() => {
    try {
      localStorage.removeItem(LEGACY_ADMIN_FLAG_KEY);
    } catch {
      // Storage may be unavailable (private mode) — nothing to clean.
    }

    if (hasSignedOutMarker()) {
      // A previous logout was never confirmed: stay signed out and quietly retry revoking
      // the server session (clears the marker on success).
      if (statusRef.current === 'loading') setStatusBoth('unauthenticated');
      if (!pendingLogoutRef.current) void revokeServerSession(null);
    } else if (statusRef.current === 'loading') {
      refreshSession().catch(() => {
        // Network/server error on startup: show the login screen quietly.
        if (statusRef.current === 'loading') setStatusBoth('unauthenticated');
      });
    }

    let channel: BroadcastChannel | null = null;
    if (typeof BroadcastChannel !== 'undefined') {
      channel = new BroadcastChannel(BROADCAST_CHANNEL_NAME);
      channelRef.current = channel;
      channel.onmessage = (event: MessageEvent<BroadcastMessage>) => {
        const msg = event.data;
        if (!msg || typeof msg !== 'object') return;
        if (msg.type === 'logout') {
          if (statusRef.current !== 'unauthenticated' || accessTokenRef.current) {
            clearLocal(msg.reason === 'session_ended' ? SESSION_ENDED_MESSAGE : null);
          }
        } else if (msg.type === 'login' && statusRef.current !== 'authenticated') {
          // Another tab signed in; restore via the (lock-serialised) refresh.
          refreshSession().catch(() => {});
        }
      };
    }

    // Timers can be throttled/frozen while the tab sleeps — re-check on wake.
    const onWake = () => {
      if (document.visibilityState === 'hidden') return;
      if (statusRef.current !== 'authenticated') return;
      const now = Date.now();
      if (sessionExpiresAtRef.current && now >= sessionExpiresAtRef.current) {
        endSessionRef.current();
      } else if (accessExpiresAtRef.current - now < REFRESH_LEAD_MS) {
        refreshSession().catch(() => {});
      }
    };
    document.addEventListener('visibilitychange', onWake);
    window.addEventListener('focus', onWake);

    return () => {
      document.removeEventListener('visibilitychange', onWake);
      window.removeEventListener('focus', onWake);
      if (channel) {
        channel.onmessage = null;
        channel.close();
      }
      if (channelRef.current === channel) channelRef.current = null;
      clearTimers();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Actions ----

  const login = useCallback(
    (email: string, password: string) => authApi.login(email.trim(), password),
    []
  );

  const verifyOtp = useCallback(
    async (challengeId: string, code: string) => {
      // Under the lock so a still-retrying logout (any tab) can't interleave with the new
      // cookie; lifting the marker inside the lock makes pending logout retries stand down.
      const session = await withRefreshLock(async () => {
        const created = await authApi.verifyOtp(challengeId, code);
        setSignedOutMarker(false);
        return created;
      });
      // A fresh session supersedes anything in flight.
      epochRef.current += 1;
      applySession(session);
      broadcast({ type: 'login' });
      return session;
    },
    [applySession, broadcast]
  );

  const resendOtp = useCallback((challengeId: string) => authApi.resendOtp(challengeId), []);

  const logout = useCallback(async () => {
    const token = accessTokenRef.current;
    clearLocal(null);
    const epoch = epochRef.current;
    const confirmed = revokeServerSession(token); // sets the signed-out marker synchronously
    broadcast({ type: 'logout' });
    if (!(await confirmed) && epoch === epochRef.current) {
      // Still signed out locally (marker blocks silent restore); warn without blocking.
      setSessionEndedReason(LOGOUT_INCOMPLETE_MESSAGE);
    }
  }, [broadcast, clearLocal, revokeServerSession]);

  const authFetch = useCallback(
    async <T,>(path: string, init: Omit<ApiRequestOptions, 'accessToken'> = {}): Promise<T> => {
      const sessionGone = () =>
        new ApiError(401, 'session_expired', 'Your admin session has ended. Please sign in again.');

      let token = accessTokenRef.current;
      if (!token || accessExpiresAtRef.current - Date.now() < 5_000) {
        const session = await refreshSession();
        // `null` can also mean a newer sign-in superseded this refresh — use its token.
        token = session?.accessToken ?? accessTokenRef.current;
        if (!token) throw sessionGone();
      }

      try {
        return await apiRequest<T>(path, { ...init, accessToken: token });
      } catch (err) {
        if (isApiError(err) && err.status === 401 && err.code === 'unauthorized') {
          const session = await refreshSession();
          const retryToken = session?.accessToken ?? accessTokenRef.current;
          if (!retryToken || retryToken === token) throw sessionGone();
          return apiRequest<T>(path, { ...init, accessToken: retryToken });
        }
        if (isApiError(err) && err.status === 401 && err.code === 'session_expired') {
          clearLocal(SESSION_REVOKED_MESSAGE);
        }
        throw err;
      }
    },
    [clearLocal, refreshSession]
  );

  const clearSessionEndedReason = useCallback(() => setSessionEndedReason(null), []);

  const value = useMemo<AdminAuthContextValue>(
    () => ({
      status,
      admin,
      sessionExpiresAt,
      sessionEndedReason,
      clearSessionEndedReason,
      login,
      verifyOtp,
      resendOtp,
      logout,
      authFetch,
    }),
    [
      status,
      admin,
      sessionExpiresAt,
      sessionEndedReason,
      clearSessionEndedReason,
      login,
      verifyOtp,
      resendOtp,
      logout,
      authFetch,
    ]
  );

  return <AdminAuthContext.Provider value={value}>{children}</AdminAuthContext.Provider>;
};

export function useAdminAuth(): AdminAuthContextValue {
  const ctx = useContext(AdminAuthContext);
  if (!ctx) throw new Error('useAdminAuth must be used within <AdminAuthProvider>');
  return ctx;
}
