/**
 * Public checkout endpoint (`POST /orders`) — see backend/API.md.
 *
 * - Prices are always taken from the DB server-side; the client only sends product ids + quantities.
 * - Stock for every line is decremented atomically, or nothing happens.
 * - Re-sending the same `idempotencyKey` returns the original order (HTTP 200) instead of
 *   creating a second one; the same key with a different payload → 409 `idempotency_conflict`.
 */
import { apiRequest } from './api';
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
    /** `pay_later` when online payments are switched off, `razorpay` otherwise (may be absent). */
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

/** `POST /orders/{orderNumber}/pay` → options for Razorpay Checkout. */
export interface PaymentSession {
  keyId: string;
  razorpayOrderId: string;
  amountPaise: number;
  currency: string;
  name: string;
  description: string;
  prefill: { name?: string; email?: string | null; contact?: string };
  notes: Record<string, string>;
  paymentExpiresAt: string | null;
  testMode: boolean;
}

export interface VerifyPaymentInput {
  razorpayOrderId: string;
  razorpayPaymentId: string;
  razorpaySignature: string;
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

/** Creates (first call) or reuses the Razorpay order for this order. */
export function startOrderPayment(orderNumber: string, orderToken: string): Promise<PaymentSession> {
  return apiRequest<PaymentSession>(`${orderPath(orderNumber)}/pay`, {
    method: 'POST',
    headers: tokenHeader(orderToken),
  });
}

/** Sends Checkout's `handler` response for server-side verification. Idempotent. */
export async function verifyOrderPayment(
  orderNumber: string,
  orderToken: string,
  body: VerifyPaymentInput
): Promise<GuestOrder> {
  const res = await apiRequest<GuestOrder>(`${orderPath(orderNumber)}/pay/verify`, {
    method: 'POST',
    headers: tokenHeader(orderToken),
    body,
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
