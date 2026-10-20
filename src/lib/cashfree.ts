/**
 * Loads the Cashfree JS SDK v3 on demand. The script is injected once and the promise is
 * cached; a failed/timed-out load clears the cache so the next attempt retries cleanly.
 */
const SDK_SRC = 'https://sdk.cashfree.com/js/v3/cashfree.js';
const LOAD_TIMEOUT_MS = 15_000;

let loading: Promise<CashfreeFactory> | null = null;

export class CashfreeLoadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CashfreeLoadError';
  }
}

export function loadCashfreeSdk(): Promise<CashfreeFactory> {
  if (typeof window.Cashfree === 'function') return Promise.resolve(window.Cashfree);
  if (loading) return loading;

  loading = new Promise<CashfreeFactory>((resolve, reject) => {
    const script = document.createElement('script');
    script.src = SDK_SRC;
    script.async = true;
    script.dataset.cashfreeSdk = 'true';

    const fail = (message: string) => {
      window.clearTimeout(timer);
      script.remove();
      loading = null;
      reject(new CashfreeLoadError(message));
    };

    const timer = window.setTimeout(
      () => fail('The payment window took too long to load. Check your connection and try again.'),
      LOAD_TIMEOUT_MS
    );

    script.onload = () => {
      window.clearTimeout(timer);
      if (typeof window.Cashfree === 'function') {
        resolve(window.Cashfree);
      } else {
        fail('The payment window could not be started. Please try again.');
      }
    };
    script.onerror = () =>
      fail(
        "Couldn't load the secure payment window. Check your connection (or disable content blockers for this site) and try again."
      );

    document.head.appendChild(script);
  });

  return loading;
}

/**
 * Opens Cashfree Checkout as a pop-up on this page. Resolves when the pop-up closes (paid, failed
 * or dismissed); in in-app browsers the SDK may redirect to the server's `return_url` instead.
 */
export async function openCashfreeCheckout(
  mode: CashfreeSdkMode,
  paymentSessionId: string
): Promise<CashfreeCheckoutResult> {
  const factory = await loadCashfreeSdk();
  let instance: CashfreeInstance;
  try {
    instance = factory({ mode });
  } catch {
    throw new CashfreeLoadError('The payment window could not be started. Please try again.');
  }
  const result = await instance.checkout({ paymentSessionId, redirectTarget: '_modal' });
  // Defensive: an unexpected SDK build might resolve with nothing.
  return result ?? {};
}
