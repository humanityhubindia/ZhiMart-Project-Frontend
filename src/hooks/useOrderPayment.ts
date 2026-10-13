import { useCallback, useEffect, useRef, useState } from 'react';
import { isApiError } from '../lib/api';
import { GuestOrder, startOrderPayment, verifyOrderPayment } from '../lib/ordersApi';
import { loadRazorpayCheckout } from '../lib/razorpay';

/**
 * - `idle`: nothing in progress
 * - `preparing`: creating/reusing the Razorpay order and loading Checkout
 * - `open`: Razorpay Checkout modal is open
 * - `verifying`: Checkout reported success; server is verifying the signature + payment
 * - `confirming`: waiting for the server (webhook) to settle the payment — the page polls
 * - `confirm_timeout`: still unsettled after polling for a while
 */
export type PaymentPhase = 'idle' | 'preparing' | 'open' | 'verifying' | 'confirming' | 'confirm_timeout';

export interface PaymentNotice {
  tone: 'error' | 'info';
  message: string;
}

const CONFIRM_POLL_WINDOW_MS = 2 * 60_000;

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
  /** Last `payment.failed` reason from Checkout (cleared when a new attempt starts). */
  const [failureReason, setFailureReason] = useState<string | null>(null);

  const phaseRef = useRef<PaymentPhase>('idle');
  const rzpRef = useRef<RazorpayInstance | null>(null);
  const confirmTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  const setPhase = useCallback((next: PaymentPhase) => {
    phaseRef.current = next;
    if (mountedRef.current) setPhaseState(next);
  }, []);

  const clearConfirmTimer = () => {
    if (confirmTimerRef.current !== null) {
      window.clearTimeout(confirmTimerRef.current);
      confirmTimerRef.current = null;
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearConfirmTimer();
      try {
        rzpRef.current?.close();
      } catch {
        // Checkout already closed.
      }
    };
  }, []);

  /** Poll the order (the page drives polling while phase === 'confirming') for up to 2 minutes. */
  const startConfirming = useCallback(() => {
    clearConfirmTimer();
    setPhase('confirming');
    confirmTimerRef.current = window.setTimeout(() => {
      if (phaseRef.current === 'confirming') setPhase('confirm_timeout');
    }, CONFIRM_POLL_WINDOW_MS);
  }, [setPhase]);

  /** The page calls this once the server shows the order is no longer awaiting payment. */
  const settle = useCallback(() => {
    clearConfirmTimer();
    if (phaseRef.current !== 'open') setPhase('idle');
  }, [setPhase]);

  const handleVerify = useCallback(
    async (resp: RazorpaySuccessResponse) => {
      if (!orderToken) return;
      setPhase('verifying');
      setFailureReason(null);
      setNotice(null);
      try {
        const order = await verifyOrderPayment(orderNumber, orderToken, {
          razorpayOrderId: resp.razorpay_order_id,
          razorpayPaymentId: resp.razorpay_payment_id,
          razorpaySignature: resp.razorpay_signature,
        });
        onOrderUpdate(order);
        setPhase('idle');
      } catch (err) {
        const code = isApiError(err) ? err.code : null;
        if (code === 'order_already_paid') {
          setPhase('idle');
          onRefresh();
        } else if (code === 'online_payments_disabled') {
          setPhase('idle');
          onPaymentsDisabled();
        } else if (code === 'order_expired') {
          setPhase('idle');
          setNotice({
            tone: 'error',
            message:
              'The payment window had already closed, so this order could not be completed. If money was deducted, it will be refunded automatically.',
          });
          onRefresh();
        } else if (code === 'payment_verification_failed') {
          setNotice({
            tone: 'error',
            message:
              "We couldn't confirm this payment. If money was deducted it will be confirmed automatically or refunded.",
          });
          startConfirming();
        } else {
          // Network error / timeout / 5xx: the webhook may still settle it — never report failure here.
          startConfirming();
        }
      }
    },
    [orderNumber, orderToken, onOrderUpdate, onRefresh, onPaymentsDisabled, setPhase, startConfirming]
  );

  const pay = useCallback(async () => {
    if (!orderToken) return;
    if (phaseRef.current === 'preparing' || phaseRef.current === 'open' || phaseRef.current === 'verifying') return;

    setPhase('preparing');
    setNotice(null);
    setFailureReason(null);

    let session;
    try {
      session = await startOrderPayment(orderNumber, orderToken);
    } catch (err) {
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

    let Razorpay: RazorpayConstructor;
    try {
      Razorpay = await loadRazorpayCheckout();
    } catch (err) {
      setPhase('idle');
      setNotice({
        tone: 'error',
        message: err instanceof Error ? err.message : "Couldn't load the payment window. Please try again.",
      });
      return;
    }
    if (!mountedRef.current) return;

    const rzp = new Razorpay({
      key: session.keyId,
      order_id: session.razorpayOrderId,
      amount: session.amountPaise,
      currency: session.currency,
      name: session.name,
      description: session.description,
      prefill: {
        name: session.prefill?.name ?? undefined,
        email: session.prefill?.email ?? undefined,
        contact: session.prefill?.contact ?? undefined,
      },
      notes: session.notes,
      theme: { color: '#0f172a' },
      retry: { enabled: true },
      modal: {
        confirm_close: true,
        ondismiss: () => {
          if (phaseRef.current !== 'open') return; // handler already took over
          setPhase('idle');
          onRefresh();
        },
      },
      handler: (resp) => {
        void handleVerify(resp);
      },
    });
    rzp.on('payment.failed', (resp) => {
      const reason =
        resp?.error?.description ||
        resp?.error?.reason?.replace(/_/g, ' ') ||
        'The payment did not go through.';
      if (mountedRef.current) setFailureReason(reason);
    });
    rzpRef.current = rzp;
    setPhase('open');
    rzp.open();
  }, [orderNumber, orderToken, onRefresh, onPaymentsDisabled, setPhase, handleVerify]);

  return {
    phase,
    notice,
    failureReason,
    pay,
    settle,
    startConfirming,
    dismissNotice: () => setNotice(null),
  };
}
