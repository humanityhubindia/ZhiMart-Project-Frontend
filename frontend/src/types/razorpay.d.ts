// Minimal typings for Razorpay Standard Checkout (https://checkout.razorpay.com/v1/checkout.js).
// The script is injected at runtime (src/lib/razorpay.ts) and never bundled.

interface RazorpaySuccessResponse {
  razorpay_payment_id: string;
  razorpay_order_id: string;
  razorpay_signature: string;
}

interface RazorpayFailureResponse {
  error: {
    code?: string;
    description?: string;
    source?: string;
    step?: string;
    reason?: string;
    metadata?: { order_id?: string; payment_id?: string };
  };
}

interface RazorpayCheckoutOptions {
  key: string;
  order_id: string;
  amount: number;
  currency: string;
  name: string;
  description?: string;
  image?: string;
  prefill?: { name?: string; email?: string; contact?: string };
  notes?: Record<string, string>;
  theme?: { color?: string };
  retry?: { enabled: boolean; max_count?: number };
  timeout?: number;
  modal?: {
    ondismiss?: () => void;
    confirm_close?: boolean;
    escape?: boolean;
    backdropclose?: boolean;
  };
  handler: (response: RazorpaySuccessResponse) => void;
}

interface RazorpayInstance {
  open(): void;
  close(): void;
  on(event: 'payment.failed', callback: (response: RazorpayFailureResponse) => void): void;
}

interface RazorpayConstructor {
  new (options: RazorpayCheckoutOptions): RazorpayInstance;
}

interface Window {
  Razorpay?: RazorpayConstructor;
}
