import { useCallback, useEffect, useRef, useState } from 'react';
import { isApiError } from '../lib/api';
import { loadCashfreeSdk, openCashfreeCheckout } from '../lib/cashfree';
import {
  GuestOrder,
  rememberOrderToken,
  startOrderPayment,
  verifyOrderPayment,
} from '../lib/ordersApi';

/**
 * - `idle`: nothing in progress
 * - `preparing`: creating/reusing the Cashfree order and loading the SDK
 * - `open`: Cashfree Checkout pop-up is open
 * - `verifying`: Checkout closed; the server is checking the payment with Cashfree
 * - `confirming`: Cashfree reports the payment as still processing — the hook polls verify
 * - `confirm_timeout`: still unsettled after polling for a while
 */
export type PaymentPhase = 'idle' | 'preparing' | 'open' | 'verifying' | 'confirming' | 'confirm_timeout';

export interface PaymentNotice {
  tone: 'error' | 'info';
  message: string;
}

/** How long to keep polling verify while a payment is processing (UPI collect can take a while). */
const CONFIRM_POLL_WINDOW_MS = 2 * 60_000;
/** Verify poll interval — well inside the server's per-order rate limit. */
const CONFIRM_POLL_INTERVAL_MS = 5_000;

const NOT_COMPLETED_MESSAGE = 'The payment was not completed. You can try again.';

/** Phases in which a new checkout must never be opened (it could charge the customer twice). */
const BUSY_PHASES: readonly PaymentPhase[] = ['preparing', 'open', 'verifying', 'confirming'];

/**
 * What Checkout reported — used only to word the message. The outcome itself always comes from
 * the server, which reads Cashfree's API (`paymentDetails` arrives for failed attempts too).
 */
interface CheckoutOutcome {
  /** The customer finished an attempt (`paymentDetails`), as opposed to closing the pop-up. */
  completedAttempt: boolean;
  message: string | null;
}

const cleanMessage = (raw: unknown): string | null => {
  if (typeof raw !== 'string') return null;
  const msg = raw.trim().replace(/[.\s]+$/, '');
  return msg ? msg.slice(0, 200) : null;
};

function outcomeOf(result: CashfreeCheckoutResult): CheckoutOutcome {
  if (result.paymentDetails) {
    return { completedAttempt: true, message: cleanMessage(result.paymentDetails.paymentMessage) };
  }
  return { completedAttempt: false, message: cleanMessage(result.error?.message) };
}

interface UseOrderPaymentOptions {
  orderNumber: string;
  orderToken: string | null;
  /** Called with the authoritative order after a successful verify. */
  onOrderUpdate: (order: GuestOrder) => void;
  /** Re-read the order from the server (e.g. after `order_already_paid` / `order_expired`). */
  onRefresh: () => void;
  /** Server says online payments are switched off (`online_payments_disabled`). */
  onPaymentsDisabled: () => void;
}

export function useOrderPayment({
  orderNumber,
  orderToken,
  onOrderUpdate,
  onRefresh,
  onPaymentsDisabled,
}: UseOrderPaymentOptions) {
  const [phase, setPhaseState] = useState<PaymentPhase>('idle');
  const [notice, setNotice] = useState<PaymentNotice | null>(null);
  /** Why the last completed attempt failed (cleared when a new attempt starts). */
  const [failureReason, setFailureReason] = useState<string | null>(null);

  const phaseRef = useRef<PaymentPhase>('idle');
  const mountedRef = useRef(true);
  /**
   * Bumped by every new attempt / check / settle / unmount. Async work captures the value it
   * started with and drops its result when it no longer matches, so a slow response from an
   * earlier attempt can never overwrite a newer state (or touch an unmounted component).
   */
  const runIdRef = useRef(0);
  const pollTimerRef = useRef<number | null>(null);
  /** End of the current confirming window (ms epoch). */
  const confirmDeadlineRef = useRef(0);

  const setPhase = useCallback((next: PaymentPhase) => {
    phaseRef.current = next;
    if (mountedRef.current) setPhaseState(next);
  }, []);

  const clearPollTimer = () => {
    if (pollTimerRef.current !== null) {
      window.clearTimeout(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      runIdRef.current += 1;
      clearPollTimer();
      // The Cashfree SDK has no close() — a pop-up still open is simply orphaned; its result is
      // ignored (stale run) and the order page re-reads the server state next time.
    };
  }, []);

  // The poll timer calls the latest verify through a ref (verify ⇄ poll would otherwise be circular).
  const verifyRef = useRef<(runId: number, outcome: CheckoutOutcome | null) => Promise<void>>(async () => {});

  /**
   * Keep (or start) confirming and schedule the next verify. Polls are chained — the next one is
   * scheduled only after the previous response — so requests never overlap.
   */
  const pollAgain = useCallback(
    (runId: number, delayMs: number = CONFIRM_POLL_INTERVAL_MS) => {
      clearPollTimer();
      if (phaseRef.current !== 'confirming') {
        confirmDeadlineRef.current = Date.now() + CONFIRM_POLL_WINDOW_MS;
        setPhase('confirming');
      }
      const remaining = confirmDeadlineRef.current - Date.now();
      if (remaining <= 0) {
        setPhase('confirm_timeout');
        return;
      }
      pollTimerRef.current = window.setTimeout(() => {
        pollTimerRef.current = null;
        if (runId !== runIdRef.current || phaseRef.current !== 'confirming') return;
        // At the deadline this is the final check; a still-pending answer then times out above.
        void verifyRef.current(runId, null);
      }, Math.min(delayMs, remaining));
    },
    [setPhase]
  );

  /** Stops confirming and returns to idle. */
  const finish = useCallback(() => {
    clearPollTimer();
    setPhase('idle');
  }, [setPhase]);

  const verify = useCallback(
    async (runId: number, outcome: CheckoutOutcome | null) => {
      if (!orderToken) return;
      const isStale = () => !mountedRef.current || runId !== runIdRef.current;
      try {
        const order = await verifyOrderPayment(orderNumber, orderToken);
        if (isStale()) return;
        setNotice(null);
        setFailureReason(null);
        onOrderUpdate(order);
        finish();
      } catch (err) {
        if (isStale()) return;
        const code = isApiError(err) ? err.code : null;
        if (code === 'payment_pending') {
          pollAgain(runId);
        } else if (code === 'payment_not_completed') {
          finish();
          if (outcome?.completedAttempt) {
            // Shown as "Payment failed: <reason>. If any amount was deducted…"
            setFailureReason(outcome.message ?? 'The payment did not go through');
          } else {
            setNotice({
              tone: 'info',
              message: outcome?.message ? `The payment was not completed (${outcome.message}). You can try again.` : NOT_COMPLETED_MESSAGE,
            });
          }
        } else if (code === 'order_already_paid') {
            finish();
          onRefresh();
        } else if (code === 'online_payments_disabled') {
          finish();
          onPaymentsDisabled();
        } else if (code === 'order_expired') {
          finish();
          setNotice({
            tone: 'error',
            message:
              'The payment window had already closed, so this order could not be completed. If money was deducted, it will be refunded automatically.',
          });
          onRefresh();
        } else if (code === 'order_not_payable') {
          finish();
          setNotice({ tone: 'error', message: 'This order can no longer be paid.' });
          onRefresh();
        } else if (code === 'not_found') {
          finish();
          setNotice({ tone: 'error', message: "We couldn't find this order. Please check that you opened the full order link." });
        } else if (code === 'payment_verification_failed') {
          setNotice({
            tone: 'error',
            message:
              "We couldn't confirm this payment. If money was deducted it will be confirmed automatically or refunded.",
          });
          pollAgain(runId);
        } else if (code === 'too_many_requests') {
          const retryMs = isApiError(err) && err.retryAfter ? err.retryAfter * 1000 : 0;
          pollAgain(runId, Math.max(CONFIRM_POLL_INTERVAL_MS, retryMs));
        } else {
          // Network error / timeout / 5xx / gateway hiccup: the payment may well have succeeded
          // (a webhook can settle it too) — never report failure here, keep checking.
          pollAgain(runId);
        }
      }
    },
    [orderNumber, orderToken, onOrderUpdate, onRefresh, onPaymentsDisabled, finish, pollAgain]
  );
  verifyRef.current = verify;

  /** Restart confirming (the page's "Check again" button): verify now, then poll for 2 more minutes. */
  const startConfirming = useCallback(() => {
    if (!orderToken) return;
    if (phaseRef.current === 'preparing' || phaseRef.current === 'open' || phaseRef.current === 'verifying') return;
    const runId = ++runIdRef.current;
    clearPollTimer();
    confirmDeadlineRef.current = Date.now() + CONFIRM_POLL_WINDOW_MS;
    setPhase('confirming');
    void verify(runId, null);
  }, [orderToken, setPhase, verify]);

  /**
   * Ask the server once whether the order was paid — used when the customer comes back from a
   * Cashfree redirect (`cf_return=1`), where there is no Checkout result to go on.
   */
  const checkPayment = useCallback(() => {
    if (!orderToken || BUSY_PHASES.includes(phaseRef.current)) return;
    const runId = ++runIdRef.current;
    clearPollTimer();
    setNotice(null);
    setPhase('verifying');
    void verify(runId, null);
  }, [orderToken, setPhase, verify]);

  /** The page calls this once the server shows the order is no longer awaiting payment. */
  const settle = useCallback(() => {
    // An open pop-up resolves on its own; its verify then reports `order_already_paid` or the order.
    if (phaseRef.current === 'open') return;
    runIdRef.current += 1;
    finish();
  }, [finish]);

  const pay = useCallback(async () => {
    if (!orderToken) return;
    // phaseRef is updated synchronously, so a double click / auto-pay race can't open two pop-ups.
    if (BUSY_PHASES.includes(phaseRef.current)) return;

    const runId = ++runIdRef.current;
    const isStale = () => !mountedRef.current || runId !== runIdRef.current;
    clearPollTimer();
    setPhase('preparing');
    setNotice(null);
    setFailureReason(null);

    let session;
    try {
      session = await startOrderPayment(orderNumber, orderToken);
    } catch (err) {
      if (isStale()) return;
      setPhase('idle');
      const code = isApiError(err) ? err.code : null;
      if (code === 'online_payments_disabled') {
        onPaymentsDisabled();
        return;
      }
      if (code === 'order_already_paid' || code === 'order_expired' || code === 'order_not_payable') {
        onRefresh();
        if (code === 'order_not_payable') {
          setNotice({ tone: 'error', message: 'This order can no longer be paid.' });
        }
        return;
      }
      if (code === 'payment_gateway_error') {
        setNotice({
          tone: 'error',
          message: 'The payment service is temporarily unavailable. Please try again in a moment.',
        });
      } else if (code === 'not_found') {
        setNotice({ tone: 'error', message: "We couldn't find this order. Please check that you opened the full order link." });
      } else if (code === 'too_many_requests') {
        const wait = isApiError(err) && err.retryAfter ? ` in ${err.retryAfter} seconds` : ' in a moment';
        setNotice({ tone: 'error', message: `Too many attempts. Please try again${wait}.` });
      } else {
        setNotice({
          tone: 'error',
          message: isApiError(err) ? err.message : 'Could not start the payment. Please try again.',
        });
      }
      return;
    }
    if (isStale()) return;

    try {
      await loadCashfreeSdk();
    } catch (err) {
      if (isStale()) return;
      setPhase('idle');
      setNotice({
        tone: 'error',
        message: err instanceof Error ? err.message : "Couldn't load the payment window. Please try again.",
      });
      return;
    }
    if (isStale()) return;

    // If the SDK falls back to a full-page redirect (in-app browsers), the return URL carries no
    // token — keep it in this tab so the order page can recover it on the way back.
    rememberOrderToken(orderNumber, orderToken);
    setPhase('open');

    let result: CashfreeCheckoutResult;
    try {
      result = await openCashfreeCheckout(session.mode, session.paymentSessionId);
    } catch (err) {
      // Couldn't open (or crashed mid-way). Still ask the server — it's the source of truth.
      result = { error: { message: err instanceof Error ? err.message : undefined } };
    }
    if (isStale()) return;

    if (result.redirect && !result.error && !result.paymentDetails) {
      // The SDK is taking the customer to Cashfree's hosted page; they come back via return_url
      // (`cf_return=1`) and the page verifies then.
      setPhase('idle');
      setNotice({
        tone: 'info',
        message: "Continue your payment on Cashfree's secure page. You'll be brought back here when it's done.",
      });
      return;
    }

    // Paid, failed or closed: whatever Checkout says, the server decides.
    setPhase('verifying');
    await verify(runId, outcomeOf(result));
  }, [orderNumber, orderToken, onRefresh, onPaymentsDisabled, setPhase, verify]);

  return {
    phase,
    notice,
    failureReason,
    pay,
    settle,
    startConfirming,
    checkPayment,
    dismissNotice: () => setNotice(null),
  };
}
