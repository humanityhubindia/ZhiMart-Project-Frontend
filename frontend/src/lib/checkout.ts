/**
 * Checkout form model + client-side validation mirroring the server's `POST /orders` rules
 * (backend/API.md). The server stays authoritative; this only gives fast inline feedback.
 */
import { OrderCustomerInput } from './ordersApi';

export interface CheckoutFormValues {
  name: string;
  phone: string;
  email: string;
  address: string;
  city: string;
  pincode: string;
  notes: string;
}

export type CheckoutField = keyof CheckoutFormValues;
export type CheckoutErrors = Partial<Record<CheckoutField, string>>;

export const EMPTY_CHECKOUT_FORM: CheckoutFormValues = {
  name: '',
  phone: '',
  email: '',
  address: '',
  city: '',
  pincode: '',
  notes: '',
};

const LIMITS = {
  name: 100,
  email: 254,
  address: 500,
  city: 100,
  notes: 1000,
} as const;

/**
 * Accepts `9XXXXXXXXX`, `+91 9XXXX XXXXX`, `+91-9XXXX-XXXXX`, `09XXXXXXXXX`, `919XXXXXXXXX`.
 * Returns the bare 10-digit Indian mobile number, or null if invalid.
 */
export function normalizeIndianMobile(input: string): string | null {
  const compact = input.trim().replace(/[\s\-().]/g, '');
  const match = /^(?:\+?91|0)?([6-9]\d{9})$/.exec(compact);
  return match ? match[1] : null;
}

const PINCODE_RE = /^[1-9]\d{5}$/;
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function validateCheckout(values: CheckoutFormValues): CheckoutErrors {
  const errors: CheckoutErrors = {};
  const name = values.name.trim();
  const address = values.address.trim();
  const city = values.city.trim();
  const email = values.email.trim();
  const pincode = values.pincode.trim();

  if (!name) errors.name = 'Please enter the recipient name.';
  else if (name.length < 2) errors.name = 'Name looks too short.';
  else if (name.length > LIMITS.name) errors.name = `Name must be at most ${LIMITS.name} characters.`;

  if (!values.phone.trim()) errors.phone = 'Please enter a mobile number.';
  else if (!normalizeIndianMobile(values.phone))
    errors.phone = 'Enter a valid 10-digit Indian mobile number.';

  if (!email) errors.email = "Please enter your email — we'll send your order confirmation and tracking link there.";
  else if (email.length > LIMITS.email) errors.email = `Email must be at most ${LIMITS.email} characters.`;
  else if (!EMAIL_RE.test(email)) errors.email = 'Enter a valid email address.';

  if (!address) errors.address = 'Please enter the delivery address.';
  else if (address.length < 5) errors.address = 'Please enter the full delivery address.';
  else if (address.length > LIMITS.address)
    errors.address = `Address must be at most ${LIMITS.address} characters.`;

  if (!city) errors.city = 'Please enter the city.';
  else if (city.length > LIMITS.city) errors.city = `City must be at most ${LIMITS.city} characters.`;

  if (!pincode) errors.pincode = 'Please enter the pincode.';
  else if (!PINCODE_RE.test(pincode)) errors.pincode = 'Enter a valid 6-digit pincode.';

  if (values.notes.trim().length > LIMITS.notes)
    errors.notes = `Notes must be at most ${LIMITS.notes} characters.`;

  return errors;
}

/** The only state we deliver to (sent with every order; not asked on the form). */
export const DELIVERY_STATE = 'Tamil Nadu';

/**
 * Checkout fields the customer must fill in (mirrors `validateCheckout`) — drives the red asterisk
 * and `required` on the form. Notes are optional.
 */
export const REQUIRED_CHECKOUT_FIELDS: readonly CheckoutField[] = ['name', 'phone', 'email', 'address', 'city', 'pincode'];

export function toOrderCustomer(values: CheckoutFormValues): OrderCustomerInput {
  return {
    name: values.name.trim(),
    // Send the normalised 10-digit number; the server stores it as +91XXXXXXXXXX.
    phone: normalizeIndianMobile(values.phone) ?? values.phone.trim(),
    email: values.email.trim(),
    address: values.address.trim(),
    city: values.city.trim(),
    // Delivery is only available within Tamil Nadu, so the state is fixed.
    state: DELIVERY_STATE,
    pincode: values.pincode.trim(),
  };
}

/**
 * The single business number for phone calls and WhatsApp (country code + 10 digits, no `+`),
 * used by every "call us" / WhatsApp link on the site (display forms live in `lib/contact.ts`).
 */
export const BUSINESS_WHATSAPP_NUMBER = '919597961800';

export function openWhatsApp(message: string) {
  window.open(
    `https://wa.me/${BUSINESS_WHATSAPP_NUMBER}?text=${encodeURIComponent(message)}`,
    '_blank',
    'noopener,noreferrer'
  );
}

/**
 * WhatsApp share/support text for an order the server has already placed.
 * Never include the order page link — it carries the private access token.
 */
export function buildOrderShareMessage(
  order: {
    orderNumber: string;
    totalAmount: number;
    /** Delivery charge included in `totalAmount` (absent/0 = free). */
    deliveryFee?: number;
    mrpTotal: number;
    isTest?: boolean;
    pricingTier?: 'bulk' | 'standard';
    items: { productName: string; packInfo?: string; quantity: number; lineTotal: number }[];
  },
  customer: { name: string; city?: string },
  formatAmount: (n: number) => string,
  statusLine?: string
): string {
  const lines = order.items
    .map(
      (i, idx) =>
        `${idx + 1}. *${i.productName}*${i.packInfo ? ` (${i.packInfo})` : ''} x ${i.quantity} = ₹${formatAmount(i.lineTotal)}`
    )
    .join('\n');
  return (
    `*ZHI-MART ORDER ${order.orderNumber}*${order.isTest ? ' (TEST ORDER)' : ''}\n` +
    `---------------------------------\n` +
    `*Customer:* ${customer.name}${customer.city ? `, ${customer.city}` : ''}\n\n` +
    `*ORDER ITEMS:*\n${lines}\n\n` +
    `---------------------------------\n` +
    (order.deliveryFee !== undefined
      ? `*Delivery:* ${order.deliveryFee > 0 ? `₹${formatAmount(order.deliveryFee)}` : 'FREE'}\n`
      : '') +
    `*ORDER TOTAL:* ₹${formatAmount(order.totalAmount)}\n` +
    (order.pricingTier && !order.isTest
      ? `*Pricing:* ${order.pricingTier === 'bulk' ? 'Bulk price' : 'Standard price'}\n`
      : '') +
    (statusLine ? `*Status:* ${statusLine}\n` : '') +
    `---------------------------------\n` +
    `Hi! This is regarding my order ${order.orderNumber}.`
  );
}

/**
 * Checkout button/copy for the current payment mode. `online` is `undefined` while the storefront
 * config is loading — use neutral wording so online-payment copy never flashes in pay-later mode.
 */
export function checkoutCopy(online: boolean | undefined) {
  if (online === true) {
    return {
      action: 'Proceed to Pay',
      pending: 'Reserving your items…',
      note: 'Your items are reserved while you pay securely online via Cashfree (UPI, cards, net banking, wallets). Unpaid orders are released automatically.',
    };
  }
  if (online === false) {
    return {
      action: 'Place Order',
      pending: 'Placing your order…',
      note: 'No online payment needed now — our team will call you to confirm your order, payment and dispatch.',
    };
  }
  return { action: 'Place Order', pending: 'Placing your order…', note: null as string | null };
}

/** Field labels for mapping server `validation_error` messages (`customer.email …`) onto the form. */
const SERVER_FIELD_LABELS: Record<CheckoutField, string> = {
  name: 'Name',
  phone: 'Mobile number',
  email: 'Email',
  address: 'Address',
  city: 'City',
  pincode: 'Pincode',
  notes: 'Notes',
};

/**
 * Finds which checkout field a server validation message refers to (e.g. "customer.email is required …")
 * and returns it with a readable message, or null if it isn't about a form field.
 */
export function mapServerValidation(message: string): { field: CheckoutField; message: string } | null {
  const match = /\b(?:customer\.(name|phone|email|address|city|pincode)|(notes))\b/.exec(message);
  if (!match) return null;
  const field = (match[1] ?? match[2]) as CheckoutField;
  const readable = message.replace(match[0], SERVER_FIELD_LABELS[field]).trim();
  return { field, message: readable.charAt(0).toUpperCase() + readable.slice(1) };
}

/** `kavya@gmail.com` → `k•••@gmail.com` (for on-screen confirmation only). */
export function maskEmail(email: string): string {
  const trimmed = email.trim();
  const at = trimmed.lastIndexOf('@');
  if (at < 1) return 'your email address';
  return `${trimmed.charAt(0)}•••${trimmed.slice(at)}`;
}
