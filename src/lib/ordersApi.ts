/**
 * Public checkout endpoint (`POST /orders`) — see backend/API.md.
 *
 * - Prices are always taken from the DB server-side; the client only sends product ids + quantities.
 * - Stock for every line is decremented atomically, or nothing happens.
 * - Re-sending the same `idempotencyKey` returns the original order (HTTP 200) instead of
 *   creating a second one; the same key with a different payload → 409 `idempotency_conflict`.
 */
import { ApiError, apiRequest } from './api';
import { orderCharges } from './money';

export interface OrderCustomerInput {
  name: string;
  phone: string;
  /** Required: receives the order confirmation + tracking link. */
  email: string;
  address: string;
  city: string;
  /** Always "Tamil Nadu" — delivery is only available within Tamil Nadu. */
  state: string;
  pincode: string;
}

export interface OrderLineInput {
  productId: string;
  quantity: number;
}

export interface PlaceOrderPayload {
  idempotencyKey: string;
  customer: OrderCustomerInput;
  notes: string | null;
  items: OrderLineInput[];
}

export interface PlacedOrderItem {
  productId: string;
  productName: string;
  packInfo?: string;
  quantity: number;
  unitPrice: number;
  unitMrp?: number;
  lineTotal: number;
}

/**
 * Which price tier the server charged (two-tier pricing, see lib/money.ts): `bulk` when the order
 * reached the bulk minimum at price-list prices, otherwise `standard`. Absent on older servers.
 */
export type PricingTier = 'bulk' | 'standard';

const toPricingTier = (raw: unknown): PricingTier | undefined =>
  raw === 'bulk' || raw === 'standard' ? raw : undefined;

/** Guest order status (see backend/API.md "Order lifecycle"). */
export type GuestOrderStatus =
  | 'pending_payment'
  | 'pending_pack'
  | 'dispatched'
  | 'in_transit'
  | 'out_for_delivery'
  | 'delivered'
  | 'cancelled'
  | (string & {});

export interface PlacedOrder {
  orderNumber: string;
  status: GuestOrderStatus;
  /** When the stock reservation lapses if unpaid (RFC 3339). */
  paymentExpiresAt?: string | null;
  isTest?: boolean;
  /** Goods value after tier pricing (rupees). */
  subtotal: number;
  /** Delivery charge (rupees, 0 = free). */
  deliveryFee: number;
  /** Amount payable: `subtotal + deliveryFee`. */
  totalAmount: number;
  mrpTotal: number;
  /** Tier the server applied (authoritative — may differ from the cart's estimate). */
  pricingTier?: PricingTier;
  itemCount: number;
  createdAt: string;
  items: PlacedOrderItem[];
}

export interface PlaceOrderResult {
  order: PlacedOrder;
  /** Guest access token for pay/verify/status. Keep it ONLY in the URL fragment (`#t=`). */
  orderToken: string;
}

/** `GET /orders/{orderNumber}` (minimal PII). */
export interface GuestOrder {
  orderNumber: string;
  status: GuestOrderStatus;
  isTest: boolean;
  createdAt: string;
  paymentExpiresAt: string | null;
  payment: {
    /**
     * `cashfree` for online orders, `pay_later` when online payments are switched off, a manual
     * method set by the team, or `razorpay` on legacy orders (may be absent).
     */
    method?: string | null;
    status: 'pending' | 'paid' | 'refunded' | 'failed' | (string & {});
    instrument: string | null;
    paidAt: string | null;
  };
  /** Goods value after tier pricing (rupees). */
  subtotal: number;
  /** Delivery charge (rupees, 0 = free). */
  deliveryFee: number;
  /** Amount payable: `subtotal + deliveryFee`. */
  totalAmount: number;
  mrpTotal: number;
  /** Tier the server applied (absent on older servers). */
  pricingTier?: PricingTier;
  itemCount: number;
  items: PlacedOrderItem[];
  shipping: {
    transportPartner: string | null;
    docketNumber: string | null;
    dispatchedAt: string | null;
    deliveredAt: string | null;
  };
  customerName: string;
  city: string;
}

/** `POST /orders/{orderNumber}/pay` → what Cashfree Checkout needs to open. */
export interface PaymentSession {
  gateway: 'cashfree';
  /** Opaque, short-lived Cashfree checkout session — passed straight to `checkout()`. */
  paymentSessionId: string;
  /** Our Cashfree `order_id` (`{orderNumber}_{suffix}`), for support/debugging only. */
  gatewayOrderId: string;
  amountPaise: number;
  currency: string;
  /** SDK mode; must match the environment the session was created in. */
  mode: 'sandbox' | 'production';
  paymentExpiresAt: string | null;
  testMode: boolean;
}

/** Server limits for a single order (mirrors backend validation). */
export const ORDER_LIMITS = {
  maxLines: 50,
  maxQuantityPerLine: 1000,
} as const;

/** Fills `subtotal`/`deliveryFee` for orders placed before delivery charges existed (fee 0). */
const chargesOf = (raw: { totalAmount?: number; subtotal?: number | null; deliveryFee?: number | null } | undefined) => {
  const totalAmount = Number(raw?.totalAmount) || 0;
  const { subtotal, deliveryFee } = orderCharges({ totalAmount, subtotal: raw?.subtotal, deliveryFee: raw?.deliveryFee });
  return { totalAmount, subtotal, deliveryFee };
};

export async function placeOrder(payload: PlaceOrderPayload, signal?: AbortSignal): Promise<PlaceOrderResult> {
  const res = await apiRequest<PlaceOrderResult>('/orders', {
    method: 'POST',
    body: payload,
    signal,
  });
  const order = res?.order;
  return {
    order: {
      ...order,
      ...chargesOf(order),
      pricingTier: toPricingTier(order?.pricingTier),
      items: Array.isArray(order?.items) ? order.items : [],
    },
    orderToken: res?.orderToken ?? '',
  };
}

const orderPath = (orderNumber: string) => `/orders/${encodeURIComponent(orderNumber)}`;
const tokenHeader = (orderToken: string) => ({ 'X-Order-Token': orderToken });

function normalizeGuestOrder(raw: GuestOrder): GuestOrder {
  return {
    ...raw,
    ...chargesOf(raw),
    isTest: raw?.isTest === true,
    pricingTier: toPricingTier(raw?.pricingTier),
    paymentExpiresAt: raw?.paymentExpiresAt ?? null,
    payment: {
      method: raw?.payment?.method ?? null,
      status: raw?.payment?.status ?? 'pending',
      instrument: raw?.payment?.instrument ?? null,
      paidAt: raw?.payment?.paidAt ?? null,
    },
    items: Array.isArray(raw?.items) ? raw.items : [],
    shipping: {
      transportPartner: raw?.shipping?.transportPartner ?? null,
      docketNumber: raw?.shipping?.docketNumber ?? null,
      dispatchedAt: raw?.shipping?.dispatchedAt ?? null,
      deliveredAt: raw?.shipping?.deliveredAt ?? null,
    },
  };
}

/** `GET /orders/{orderNumber}` with the guest token. Wrong/missing token ⇒ 404 `not_found`. */
export async function fetchGuestOrder(
  orderNumber: string,
  orderToken: string,
  signal?: AbortSignal
): Promise<GuestOrder> {
  const res = await apiRequest<GuestOrder>(orderPath(orderNumber), {
    headers: tokenHeader(orderToken),
    signal,
  });
  return normalizeGuestOrder(res);
}

/**
 * Creates (first call) or reuses the Cashfree order for this order and returns a checkout session.
 * A response without a session id is treated as a gateway error rather than opening a broken modal.
 */
export async function startOrderPayment(orderNumber: string, orderToken: string): Promise<PaymentSession> {
  const res = await apiRequest<Partial<PaymentSession>>(`${orderPath(orderNumber)}/pay`, {
    method: 'POST',
    headers: tokenHeader(orderToken),
  });
  const paymentSessionId = typeof res?.paymentSessionId === 'string' ? res.paymentSessionId.trim() : '';
  if (!paymentSessionId) {
    throw new ApiError(502, 'payment_gateway_error', 'The payment service returned an incomplete response.');
  }
  return {
    gateway: 'cashfree',
    paymentSessionId,
    gatewayOrderId: res?.gatewayOrderId ?? '',
    amountPaise: Number(res?.amountPaise) || 0,
    currency: res?.currency ?? 'INR',
    mode: res?.mode === 'production' ? 'production' : 'sandbox',
    paymentExpiresAt: res?.paymentExpiresAt ?? null,
    testMode: res?.testMode === true,
  };
}

/**
 * Asks the server to check Cashfree for this order's payment and settle it (no client-side
 * signature: the server reads the gateway API). Idempotent and safe to poll.
 * - 200 ⇒ the authoritative order (paid, or already paid)
 * - 409 `payment_pending` ⇒ a payment is still processing — poll again
 * - 400 `payment_not_completed` ⇒ no successful/pending payment — the customer can retry
 */
export async function verifyOrderPayment(orderNumber: string, orderToken: string): Promise<GuestOrder> {
  const res = await apiRequest<GuestOrder>(`${orderPath(orderNumber)}/pay/verify`, {
    method: 'POST',
    headers: tokenHeader(orderToken),
    body: {},
  });
  return normalizeGuestOrder(res);
}

/**
 * Order page URL. The token goes ONLY in the fragment so it never reaches server logs,
 * analytics or the `Referer` header.
 */
export function orderPageUrl(orderNumber: string, orderToken: string): string {
  return `${window.location.pathname}?page=order&order=${encodeURIComponent(orderNumber)}#t=${encodeURIComponent(orderToken)}`;
}

/** Reads the guest token from `location.hash` (`#t=<token>`). */
export function readOrderTokenFromHash(hash: string = window.location.hash): string | null {
  const params = new URLSearchParams(hash.startsWith('#') ? hash.slice(1) : hash);
  const token = params.get('t');
  return token && token.trim() ? token.trim() : null;
}

/**
 * The Cashfree `return_url` can't carry the guest token (it would be sent to Cashfree), so before
 * opening checkout the token is stashed in this tab's sessionStorage and recovered when the
 * customer is redirected back (`?page=order&order=…&cf_return=1`, no fragment). Storage can be
 * unavailable (private mode / blocked) — every access is best-effort. The copy is tab-scoped and
 * deliberately kept until the tab closes: the page may still need it if the URL lost its fragment.
 */
const orderTokenStorageKey = (orderNumber: string) => `zm_order_token:${orderNumber}`;

export function rememberOrderToken(orderNumber: string, orderToken: string): void {
  try {
    window.sessionStorage.setItem(orderTokenStorageKey(orderNumber), orderToken);
  } catch {
    // Storage unavailable — the modal flow doesn't need it; only a redirect return would.
  }
}

export function recallOrderToken(orderNumber: string): string | null {
  try {
    const token = window.sessionStorage.getItem(orderTokenStorageKey(orderNumber));
    return token && token.trim() ? token.trim() : null;
  } catch {
    return null;
  }
}

/** RFC 4122 v4 id for `idempotencyKey`; falls back for non-secure contexts without `randomUUID`. */
export function newIdempotencyKey(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
