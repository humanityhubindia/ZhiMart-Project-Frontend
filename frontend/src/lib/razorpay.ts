/**
 * Loads Razorpay Standard Checkout on demand. The script is injected once and the promise is
 * cached; a failed/timed-out load clears the cache so the next attempt retries cleanly.
 */
const CHECKOUT_SRC = 'https://checkout.razorpay.com/v1/checkout.js';
const LOAD_TIMEOUT_MS = 15_000;

let loading: Promise<RazorpayConstructor> | null = null;

export class RazorpayLoadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RazorpayLoadError';
  }
}

export function loadRazorpayCheckout(): Promise<RazorpayConstructor> {
  if (window.Razorpay) return Promise.resolve(window.Razorpay);
  if (loading) return loading;

  loading = new Promise<RazorpayConstructor>((resolve, reject) => {
    const script = document.createElement('script');
    script.src = CHECKOUT_SRC;
    script.async = true;
    script.dataset.razorpayCheckout = 'true';

    const fail = (message: string) => {
      window.clearTimeout(timer);
      script.remove();
      loading = null;
      reject(new RazorpayLoadError(message));
    };

    const timer = window.setTimeout(
      () => fail('The payment window took too long to load. Check your connection and try again.'),
      LOAD_TIMEOUT_MS
    );

    script.onload = () => {
      window.clearTimeout(timer);
      if (window.Razorpay) {
        resolve(window.Razorpay);
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
