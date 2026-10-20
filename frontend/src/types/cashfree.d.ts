// Minimal typings for the Cashfree JS SDK v3 (https://sdk.cashfree.com/js/v3/cashfree.js).
// The script is injected at runtime (src/lib/cashfree.ts) and never bundled.
// Docs: https://www.cashfree.com/docs/payments/online/web/redirect

type CashfreeSdkMode = 'sandbox' | 'production';

interface CashfreeCheckoutOptions {
  /** `payment_session_id` from the server's Create Order call. */
  paymentSessionId: string;
  /**
   * `_modal` opens a pop-up on this page; `_self` (default) / `_top` / `_blank` redirect to the
   * hosted page; a DOM element renders inline.
   */
  redirectTarget?: '_modal' | '_self' | '_top' | '_blank' | HTMLElement;
}

/** `result.error` — set when the customer closes the pop-up or the payment errors. */
interface CashfreeCheckoutError {
  message?: string;
  code?: string;
  type?: string;
}

/**
 * Promise result of `checkout()` for pop-up / inline checkout. Exactly one field is normally set.
 * `paymentDetails` arrives for a completed attempt *whatever its outcome* — the server is always
 * the source of truth.
 */
interface CashfreeCheckoutResult {
  error?: CashfreeCheckoutError;
  /** The pop-up couldn't open here (mostly in-app browsers); the SDK redirects to `return_url`. */
  redirect?: boolean;
  paymentDetails?: { paymentMessage?: string };
}

interface CashfreeInstance {
  checkout(options: CashfreeCheckoutOptions): Promise<CashfreeCheckoutResult>;
}

type CashfreeFactory = (options: { mode: CashfreeSdkMode }) => CashfreeInstance;

interface Window {
  Cashfree?: CashfreeFactory;
}
