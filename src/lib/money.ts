/** Rupee helpers. Arithmetic is done in integer paise to avoid float drift (e.g. 165.5 × 3). */

const toPaise = (rupees: number) => Math.round((Number(rupees) || 0) * 100);

/** price × quantity, in rupees. */
export const lineTotal = (price: number, quantity: number) => (toPaise(price) * quantity) / 100;

/** Sum of rupee amounts. */
export const sumRupees = (amounts: number[]) =>
  amounts.reduce((acc, amount) => acc + toPaise(amount), 0) / 100;

const formatter = new Intl.NumberFormat('en-IN', {
  minimumFractionDigits: 0,
  maximumFractionDigits: 2,
});

/** `1650` → "1,650", `165.5` → "165.5" (prefix with ₹ at the call site). */
export const formatRupees = (rupees: number) => formatter.format(Number(rupees) || 0);

/**
 * Two-tier pricing (served by `GET /products` as `pricing`, see backend/API.md):
 * when the cart adds up to at least `bulkMinOrder` at price-list (`price`) prices, every line is
 * charged at `price` (~85% off MRP); below that, every line is charged at `standardPrice`
 * (~80% off). The server applies the same rule when the order is placed.
 *
 * Delivery: orders whose payable goods value (`subtotal`, after tier pricing) is below
 * `freeDeliveryMin` pay a flat `deliveryFee`; at or above it delivery is free. No tax (₹0 GST).
 */
export interface PricingRules {
  /** Minimum cart total, at price-list prices, that unlocks the bulk price (rupees). */
  bulkMinOrder: number;
  /** Label-only: the headline discounts of each tier. */
  bulkDiscountPercent: number;
  standardDiscountPercent: number;
  /** Flat delivery charge (rupees) for orders below `freeDeliveryMin`. */
  deliveryFee: number;
  /** Subtotal (rupees, after tier pricing) from which delivery is free (inclusive). 0 = always free. */
  freeDeliveryMin: number;
}

export const DEFAULT_PRICING: PricingRules = {
  bulkMinOrder: 5000,
  bulkDiscountPercent: 85,
  standardDiscountPercent: 80,
  deliveryFee: 500,
  freeDeliveryMin: 5000,
};

/** False when delivery is always free (`freeDeliveryMin` 0 or no fee). */
export const hasDeliveryCharge = (rules: PricingRules) => rules.freeDeliveryMin > 0 && rules.deliveryFee > 0;

/** Delivery charge for a goods subtotal (rupees): 0 when empty or at/above `freeDeliveryMin`. */
export const deliveryFeeFor = (subtotal: number, rules: PricingRules) =>
  subtotal > 0 && hasDeliveryCharge(rules) && subtotal < rules.freeDeliveryMin ? rules.deliveryFee : 0;

/** The flat fee a non-empty cart is spared once it reaches `freeDeliveryMin` (0 while it still pays it). */
export const waivedDeliveryFee = (totals: Pick<CartTotals, 'subtotal' | 'deliveryFee' | 'rules'>) =>
  totals.subtotal > 0 && totals.deliveryFee === 0 && hasDeliveryCharge(totals.rules) ? totals.rules.deliveryFee : 0;

/** False when there is no minimum (`bulkMinOrder` 0): every order gets the bulk price, nothing to unlock. */
export const hasBulkThreshold = (rules: PricingRules) => rules.bulkMinOrder > 0;

type PricedProduct = { price: number; originalPrice: number; standardPrice?: number };

/** What one unit costs in the given tier (falls back to `price` if no standard price is known). */
export const unitPriceFor = (product: PricedProduct, bulkUnlocked: boolean) =>
  bulkUnlocked ? product.price : (product.standardPrice ?? product.price);

export interface CartTotals {
  /** Goods payable after tier pricing: `listTotal` when the bulk tier is unlocked, else `standardTotal`. */
  subtotal: number;
  /** Delivery charge on top of `subtotal` (0 when free or the cart is empty). */
  deliveryFee: number;
  /** Subtotal still needed for free delivery (0 once free, when empty, or when delivery is always free). */
  amountToFreeDelivery: number;
  /** Amount payable: `subtotal + deliveryFee` (tax is ₹0). */
  total: number;
  mrpTotal: number;
  /** MRP savings on goods (`mrpTotal - subtotal`); the delivery fee is not counted against it. */
  savings: number;
  units: number;
  /** Effective discount across the goods (vs MRP), rounded. */
  discountPercent: number;
  /** Cart at price-list (bulk) prices — this is what is compared with `bulkMinOrder`. */
  listTotal: number;
  /** Cart at standard (below-threshold) prices. */
  standardTotal: number;
  bulkUnlocked: boolean;
  /** Price-list value still needed to unlock the bulk tier (0 once unlocked or when empty). */
  amountToUnlock: number;
  /** 0..1 progress of `listTotal` towards `bulkMinOrder`. */
  unlockProgress: number;
  /** `standardTotal - listTotal`: what unlocking saves (or saved) on the current cart. */
  bulkSavings: number;
  rules: PricingRules;
}

export function computeTotals(
  items: { product: PricedProduct; quantity: number }[],
  rules: PricingRules = DEFAULT_PRICING
): CartTotals {
  const listTotal = sumRupees(items.map((i) => lineTotal(i.product.price, i.quantity)));
  const standardTotal = sumRupees(
    items.map((i) => lineTotal(unitPriceFor(i.product, false), i.quantity))
  );
  const bulkUnlocked = listTotal > 0 && listTotal >= rules.bulkMinOrder;
  const subtotal = bulkUnlocked ? listTotal : standardTotal;
  const deliveryFee = deliveryFeeFor(subtotal, rules);
  const amountToFreeDelivery =
    deliveryFee > 0 ? Math.max(0, sumRupees([rules.freeDeliveryMin, -subtotal])) : 0;
  const total = sumRupees([subtotal, deliveryFee]);
  const mrpTotal = sumRupees(items.map((i) => lineTotal(i.product.originalPrice, i.quantity)));
  const savings = Math.max(0, sumRupees([mrpTotal, -subtotal]));
  const units = items.reduce((acc, i) => acc + i.quantity, 0);
  const discountPercent = mrpTotal > 0 ? Math.round((savings / mrpTotal) * 100) : 0;
  const amountToUnlock =
    bulkUnlocked || listTotal <= 0 ? 0 : Math.max(0, sumRupees([rules.bulkMinOrder, -listTotal]));
  const unlockProgress =
    rules.bulkMinOrder > 0 ? Math.min(1, Math.max(0, listTotal / rules.bulkMinOrder)) : 1;
  const bulkSavings = Math.max(0, sumRupees([standardTotal, -listTotal]));
  return {
    subtotal,
    deliveryFee,
    amountToFreeDelivery,
    total,
    mrpTotal,
    savings,
    units,
    discountPercent,
    listTotal,
    standardTotal,
    bulkUnlocked,
    amountToUnlock,
    unlockProgress,
    bulkSavings,
    rules,
  };
}

/** Shown under the totals: there is no tax on orders (₹0 GST). */
export const NO_TAX_NOTE = 'No tax · ₹0 GST';

/** "Add ₹X more for FREE delivery (free on orders ₹5,000+)", or null when delivery is already free. */
export function freeDeliveryHint(totals: Pick<CartTotals, 'deliveryFee' | 'amountToFreeDelivery' | 'rules'>): string | null {
  if (totals.deliveryFee <= 0 || totals.amountToFreeDelivery <= 0) return null;
  return `Add ₹${formatRupees(totals.amountToFreeDelivery)} more for FREE delivery (free on orders ₹${formatRupees(
    totals.rules.freeDeliveryMin
  )}+)`;
}

/**
 * Billing split of a placed order. `totalAmount` already includes the delivery fee; orders placed
 * before delivery charges have no `subtotal`/`deliveryFee` and are treated as fee 0.
 */
export function orderCharges(order: { totalAmount: number; subtotal?: number | null; deliveryFee?: number | null }) {
  const fee = Number(order.deliveryFee ?? 0);
  const deliveryFee = Number.isFinite(fee) && fee > 0 ? fee : 0;
  const sub = order.subtotal == null ? NaN : Number(order.subtotal);
  const subtotal = Number.isFinite(sub) ? sub : sumRupees([order.totalAmount, -deliveryFee]);
  return { subtotal, deliveryFee, total: order.totalAmount };
}
