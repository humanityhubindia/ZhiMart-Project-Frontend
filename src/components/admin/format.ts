import { isApiError } from '../../lib/api';
import { BUSINESS_ADDRESS } from '../../lib/contact';
import type {
  InventoryStatus,
  Order,
  OrderStatus,
  PaymentMethod,
  PaymentStatus,
  StockMovementReason,
} from '../../types/admin';
import { ORDER_PIPELINE } from '../../types/admin';

// ---------------------------------------------------------------------------
// Currency & numbers
// ---------------------------------------------------------------------------

const inrWhole = new Intl.NumberFormat('en-IN', {
  style: 'currency',
  currency: 'INR',
  minimumFractionDigits: 0,
  maximumFractionDigits: 0,
});
const inrPaise = new Intl.NumberFormat('en-IN', {
  style: 'currency',
  currency: 'INR',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});
const inrCompact = new Intl.NumberFormat('en-IN', {
  style: 'currency',
  currency: 'INR',
  notation: 'compact',
  maximumFractionDigits: 1,
});
const intFmt = new Intl.NumberFormat('en-IN');

/** ₹1,650 for whole rupees, ₹165.50 when paise are present. */
export function formatINR(amount: number | null | undefined): string {
  const n = Number(amount ?? 0);
  if (!Number.isFinite(n)) return '—';
  return Number.isInteger(Math.round(n * 100) / 100) ? inrWhole.format(n) : inrPaise.format(n);
}

/**
 * Standard (below-threshold) price preview: MRP × (100 − standardDiscountPercent)% rounded up to a
 * whole rupee but never above the MRP, and never below the price-list price (same as the server's
 * `money::standard_price_paise`, which is authoritative — `standardPrice`).
 */
export function standardPriceFor(price: number, mrp: number, standardDiscountPercent: number): number | null {
  if (!(price > 0) || !(mrp > 0)) return null;
  // Work in hundredths of a paisa-percent to dodge float drift (e.g. 210 × 0.3).
  const raw = Math.ceil(Math.round(mrp * 100 * (100 - standardDiscountPercent)) / 10000);
  return Math.max(price, Math.min(mrp, raw));
}

/** ₹1.2L style for tight chart labels. */
export function formatINRCompact(amount: number): string {
  return inrCompact.format(amount);
}

export function formatCount(n: number | null | undefined): string {
  return intFmt.format(Number(n ?? 0));
}

// ---------------------------------------------------------------------------
// Dates (always Asia/Kolkata)
// ---------------------------------------------------------------------------

const IST = 'Asia/Kolkata';
const dateTimeFmt = new Intl.DateTimeFormat('en-IN', {
  timeZone: IST,
  day: '2-digit',
  month: 'short',
  year: 'numeric',
  hour: 'numeric',
  minute: '2-digit',
  hour12: true,
});
const dateFmt = new Intl.DateTimeFormat('en-IN', {
  timeZone: IST,
  day: '2-digit',
  month: 'short',
  year: 'numeric',
});
const timeFmt = new Intl.DateTimeFormat('en-IN', {
  timeZone: IST,
  hour: 'numeric',
  minute: '2-digit',
  hour12: true,
});
const dayLabelFmt = new Intl.DateTimeFormat('en-IN', { timeZone: IST, day: 'numeric', month: 'short' });
const weekdayFmt = new Intl.DateTimeFormat('en-IN', { timeZone: IST, weekday: 'short' });

function toDate(iso: string | null | undefined): Date | null {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? null : d;
}

export function formatDateTime(iso: string | null | undefined): string {
  const d = toDate(iso);
  return d ? `${dateTimeFmt.format(d)} IST` : '—';
}

export function formatDate(iso: string | null | undefined): string {
  const d = toDate(iso);
  return d ? dateFmt.format(d) : '—';
}

export function formatTime(iso: string | null | undefined): string {
  const d = toDate(iso);
  return d ? timeFmt.format(d) : '—';
}

/** `2026-10-13` (an IST calendar day) → `13 Oct` / `Tue`. */
export function formatDayLabel(ymd: string): { day: string; weekday: string } {
  const d = new Date(`${ymd}T12:00:00+05:30`);
  if (Number.isNaN(d.getTime())) return { day: ymd, weekday: '' };
  return { day: dayLabelFmt.format(d), weekday: weekdayFmt.format(d) };
}

export function formatRelative(ms: number | null, now = Date.now()): string {
  if (!ms) return 'never';
  const secs = Math.max(0, Math.round((now - ms) / 1000));
  if (secs < 5) return 'just now';
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  return `${Math.round(mins / 60)}h ago`;
}

// ---------------------------------------------------------------------------
// Amount in words (Indian numbering)
// ---------------------------------------------------------------------------

export function numberToWords(num: number): string {
  const a = [
    '', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine',
    'Ten', 'Eleven', 'Twelve', 'Thirteen', 'Fourteen', 'Fifteen', 'Sixteen',
    'Seventeen', 'Eighteen', 'Nineteen',
  ];
  const b = ['', '', 'Twenty', 'Thirty', 'Forty', 'Fifty', 'Sixty', 'Seventy', 'Eighty', 'Ninety'];

  function inWords(n: number): string {
    if (n < 20) return a[n];
    if (n < 100) return b[Math.floor(n / 10)] + (n % 10 !== 0 ? ' ' + a[n % 10] : '');
    if (n < 1000) return a[Math.floor(n / 100)] + ' Hundred' + (n % 100 !== 0 ? ' ' + inWords(n % 100) : '');
    if (n < 100000) return inWords(Math.floor(n / 1000)) + ' Thousand' + (n % 1000 !== 0 ? ' ' + inWords(n % 1000) : '');
    if (n < 10000000) return inWords(Math.floor(n / 100000)) + ' Lakh' + (n % 100000 !== 0 ? ' ' + inWords(n % 100000) : '');
    return inWords(Math.floor(n / 10000000)) + ' Crore' + (n % 10000000 !== 0 ? ' ' + inWords(n % 10000000) : '');
  }

  const totalPaise = Math.round(Math.max(0, num) * 100);
  const rupees = Math.floor(totalPaise / 100);
  const paise = totalPaise % 100;
  const rupeeWords = rupees === 0 ? 'Zero' : inWords(rupees);
  return paise > 0 ? `${rupeeWords} Rupees and ${inWords(paise)} Paise Only` : `${rupeeWords} Rupees Only`;
}

// ---------------------------------------------------------------------------
// Labels & badge styles
// ---------------------------------------------------------------------------

export const ORDER_STATUS_META: Record<OrderStatus, { label: string; short: string; className: string }> = {
  pending_payment: { label: 'Awaiting payment', short: 'Awaiting payment', className: 'bg-sky-50 text-sky-800 border-sky-200' },
  pending_pack: { label: 'Pending Pack', short: 'Pending', className: 'bg-amber-50 text-amber-800 border-amber-200' },
  dispatched: { label: 'Dispatched', short: 'Dispatched', className: 'bg-blue-50 text-blue-800 border-blue-200' },
  in_transit: { label: 'In Transit', short: 'In transit', className: 'bg-purple-50 text-purple-800 border-purple-200' },
  out_for_delivery: { label: 'Out for Delivery', short: 'Out for delivery', className: 'bg-orange-50 text-orange-800 border-orange-200' },
  delivered: { label: 'Delivered', short: 'Delivered', className: 'bg-emerald-50 text-emerald-800 border-emerald-200' },
  cancelled: { label: 'Cancelled', short: 'Cancelled', className: 'bg-slate-100 text-slate-600 border-slate-300' },
};

export const PAYMENT_STATUS_META: Record<PaymentStatus, { label: string; className: string }> = {
  pending: { label: 'Payment pending', className: 'bg-amber-50 text-amber-800 border-amber-200' },
  paid: { label: 'Paid', className: 'bg-emerald-50 text-emerald-800 border-emerald-200' },
  failed: { label: 'Failed', className: 'bg-rose-50 text-rose-700 border-rose-200' },
  refunded: { label: 'Refunded', className: 'bg-slate-100 text-slate-700 border-slate-300' },
};

export const PAYMENT_METHOD_LABEL: Record<PaymentMethod, string> = {
  razorpay: 'Razorpay (online)',
  pay_later: 'Pay later',
  upi: 'UPI',
  card: 'Card',
  netbanking: 'Net banking',
  cod: 'Cash on delivery',
  bank_transfer: 'Bank transfer',
};

export const INVENTORY_STATUS_META: Record<InventoryStatus, { label: string; className: string }> = {
  in_stock: { label: 'In stock', className: 'bg-emerald-50 text-emerald-800 border-emerald-200' },
  low_stock: { label: 'Low stock', className: 'bg-amber-50 text-amber-800 border-amber-200' },
  out_of_stock: { label: 'Out of stock', className: 'bg-rose-50 text-rose-700 border-rose-200' },
};

export const MOVEMENT_REASON_LABEL: Record<StockMovementReason, string> = {
  initial: 'Initial stock',
  order_placed: 'Order placed',
  order_cancelled: 'Order cancelled (restocked)',
  payment_expired: 'Payment expired (released)',
  payment_revived: 'Late payment (re-reserved)',
  manual_set: 'Stock count set',
  manual_adjust: 'Write-off / removal',
  restock: 'Restock',
};

const INSTRUMENT_LABEL: Record<string, string> = {
  upi: 'UPI',
  card: 'Card',
  netbanking: 'Net banking',
  wallet: 'Wallet',
  emi: 'EMI',
  paylater: 'Pay later (Razorpay)',
  bank_transfer: 'Bank transfer',
};

export function instrumentLabel(instrument: string | null | undefined): string | null {
  if (!instrument) return null;
  return INSTRUMENT_LABEL[instrument] ?? instrument.replace(/_/g, ' ');
}

/** "Razorpay · UPI" or "Bank transfer". */
export function paymentMethodSummary(p: { method: PaymentMethod; instrument: string | null }): string {
  const method = PAYMENT_METHOD_LABEL[p.method] ?? p.method;
  const inst = instrumentLabel(p.instrument);
  return p.method === 'razorpay' ? (inst ? `Razorpay · ${inst}` : 'Razorpay') : method;
}

export const razorpayPaymentUrl = (paymentId: string) =>
  `https://dashboard.razorpay.com/app/payments/${encodeURIComponent(paymentId)}`;
export const razorpayOrderUrl = (orderId: string) =>
  `https://dashboard.razorpay.com/app/orders/${encodeURIComponent(orderId)}`;

/** Remaining refundable amount in rupees (2-dp safe). */
export function refundableAmount(order: { totalAmount: number; payment: { amountRefunded: number } }): number {
  return Math.max(0, Math.round((order.totalAmount - (order.payment.amountRefunded ?? 0)) * 100) / 100);
}

/** "12m 05s left" / "expired". */
export function formatCountdown(iso: string | null | undefined, now = Date.now()): { label: string; expired: boolean; urgent: boolean } {
  if (!iso) return { label: '—', expired: false, urgent: false };
  const ms = Date.parse(iso) - now;
  if (Number.isNaN(ms)) return { label: '—', expired: false, urgent: false };
  if (ms <= 0) return { label: 'Expired — releasing stock', expired: true, urgent: true };
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const sec = total % 60;
  const label = h > 0 ? `${h}h ${String(m).padStart(2, '0')}m left` : `${m}m ${String(sec).padStart(2, '0')}s left`;
  return { label, expired: false, urgent: ms < 5 * 60_000 };
}

export function movementReasonLabel(reason: string): string {
  return (MOVEMENT_REASON_LABEL as Record<string, string>)[reason] ?? reason.replace(/_/g, ' ');
}

/** Valid forward targets from the current status (skipping ahead is allowed). */
export function nextOrderStatuses(current: OrderStatus): OrderStatus[] {
  const idx = ORDER_PIPELINE.indexOf(current);
  if (idx < 0) return [];
  return ORDER_PIPELINE.slice(idx + 1);
}

export function requiresShipping(status: OrderStatus): boolean {
  return status !== 'pending_payment' && status !== 'pending_pack' && status !== 'cancelled';
}

export function itemsSummary(order: Order): string {
  if (!order.items.length) return `${order.itemCount} item(s)`;
  const [first, ...rest] = order.items;
  const head = `${first.productName} ×${first.quantity}`;
  return rest.length ? `${head} +${rest.length} more` : head;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

export function errorMessage(err: unknown, fallback = 'Something went wrong. Please try again.'): string {
  if (isApiError(err)) return err.message || fallback;
  if (err instanceof Error && err.message) return err.message;
  return fallback;
}

export const ORDER_CONFLICT_MESSAGE = 'This order was updated elsewhere — refreshed. Review it and try again.';

/** Message for order mutations, with the conflict wording the ops team expects. */
export function orderErrorMessage(err: unknown): string {
  if (isApiError(err) && err.code === 'version_conflict') return ORDER_CONFLICT_MESSAGE;
  return errorMessage(err);
}

// ---------------------------------------------------------------------------
// WhatsApp
// ---------------------------------------------------------------------------

export function whatsappLink(order: Order): string | null {
  const digits = order.customer.phone.replace(/\D/g, '');
  if (digits.length < 10) return null;
  const s = order.shipping;
  const lines = [
    `*Zhi-Mart Fireworks — Order Update*`,
    `Order: *${order.orderNumber}*`,
    `Hi ${order.customer.name},`,
    `Status: *${ORDER_STATUS_META[order.status].label}*`,
  ];
  if (order.status === 'cancelled' && order.cancelReason) lines.push(`Reason: ${order.cancelReason}`);
  if (s.transportPartner) lines.push(`Transport: *${s.transportPartner}*`);
  if (s.docketNumber) lines.push(`Docket / LR No: *${s.docketNumber}*`);
  if (s.dispatchedAt) lines.push(`Dispatched: ${formatDateTime(s.dispatchedAt)}`);
  lines.push(
    `Items: ${order.itemCount} (${order.items.length} product${order.items.length === 1 ? '' : 's'})`,
    `Order total: ${formatINR(order.totalAmount)}`,
    `Payment: ${order.status === 'pending_payment' ? 'Awaiting online payment' : PAYMENT_STATUS_META[order.payment.status].label}`,
    ``,
    `Thank you for shopping with Zhi-Mart Fireworks, Sivakasi.`
  );
  return `https://wa.me/${digits}?text=${encodeURIComponent(lines.join('\n'))}`;
}

// ---------------------------------------------------------------------------
// Business profile printed on invoices
// ---------------------------------------------------------------------------

/**
 * Printed on the order slip. `gstin`/`licence` are intentionally null until the
 * business supplies real registration numbers — never print placeholders.
 */
export const BUSINESS_PROFILE = {
  name: 'ZHI-MART FIREWORKS',
  tagline: 'Direct Factory Fireworks • Sivakasi',
  address: BUSINESS_ADDRESS,
  helpline: '+91 95979 61800',
  gstin: null as string | null,
  licence: null as string | null,
};

// ---------------------------------------------------------------------------
// Product images
// ---------------------------------------------------------------------------

/** Images that actually exist in `public/assets/products/`. */
export const PRODUCT_ASSET_IMAGES = [
  { label: 'Lakshmi box', url: '/assets/products/lakshmi_box.jpg' },
  { label: 'Aerial box', url: '/assets/products/aerial_box.jpg' },
  { label: 'Chakra box', url: '/assets/products/chakra_box.jpg' },
  { label: 'Flower pots', url: '/assets/products/flower_pots.jpg' },
  { label: 'Flower pots box', url: '/assets/products/flowerpots_box.jpg' },
  { label: 'Hamper box', url: '/assets/products/hamper_box.jpg' },
  { label: 'Kurukshetra box', url: '/assets/products/kurukshetra_box.jpg' },
] as const;

/** Host part of an https URL: DNS name or IPv4, optional `:port`, no userinfo (mirrors the server). */
function isValidAuthority(authority: string): boolean {
  const idx = authority.lastIndexOf(':');
  const host = idx >= 0 ? authority.slice(0, idx) : authority;
  const port = idx >= 0 ? authority.slice(idx + 1) : null;
  const portOk = port === null || /^\d{1,5}$/.test(port);
  const hostOk =
    host.length > 0 &&
    host.length <= 253 &&
    host.split('.').every((l) => l.length > 0 && l.length <= 63 && !l.startsWith('-') && !l.endsWith('-') && /^[A-Za-z0-9-]+$/.test(l));
  return portOk && hostOk;
}

/** Mirrors server rules: `/assets/...` (letters, digits, `/._-`) or `https://host/...`, ≤ 2048 chars. Returns an error or null. */
export function validateImageRef(value: string): string | null {
  const v = value.trim();
  if (!v) return 'Image is required.';
  if (v.length > 2048) return 'Image path must be at most 2048 characters.';
  if (v.startsWith('/assets/')) {
    const rest = v.slice('/assets/'.length);
    if (!rest || rest.endsWith('/')) return 'Enter the full file path, e.g. /assets/products/box.jpg.';
    if (!/^[A-Za-z0-9/._-]+$/.test(rest)) return 'Asset paths may only use letters, digits, "/", ".", "-" and "_".';
    if (v.includes('..') || v.includes('//')) return 'Asset path must not contain ".." or "//".';
    return null;
  }
  if (v.startsWith('https://')) {
    // Printable ASCII without quotes, angle brackets, backslash, backtick, braces, pipe or caret.
    if (!/^[\x21-\x7E]+$/.test(v) || /["<>\\`{}|^]/.test(v)) return 'URL contains characters that are not allowed (spaces, quotes, <, >, {, }, |, ^, `).';
    const rest = v.slice('https://'.length);
    const end = rest.search(/[/?#]/);
    const authority = end === -1 ? rest : rest.slice(0, end);
    if (!isValidAuthority(authority)) return 'Enter a valid https:// URL with a host name.';
    return null;
  }
  return 'Use a site path starting with /assets/ or an https:// URL.';
}

// ---------------------------------------------------------------------------
// Console navigation
// ---------------------------------------------------------------------------

export type AdminTab = 'dashboard' | 'orders' | 'products' | 'payments' | 'inventory';
