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
 * (~70% off). The server applies the same rule when the order is placed.
 */
export interface PricingRules {
  /** Minimum cart total, at price-list prices, that unlocks the bulk price (rupees). */
  bulkMinOrder: number;
  /** Label-only: the headline discounts of each tier. */
  bulkDiscountPercent: number;
  standardDiscountPercent: number;
}

export const DEFAULT_PRICING: PricingRules = {
  bulkMinOrder: 10000,
  bulkDiscountPercent: 85,
  standardDiscountPercent: 70,
};

/** False when there is no minimum (`bulkMinOrder` 0): every order gets the bulk price, nothing to unlock. */
export const hasBulkThreshold = (rules: PricingRules) => rules.bulkMinOrder > 0;

type PricedProduct = { price: number; originalPrice: number; standardPrice?: number };

/** What one unit costs in the given tier (falls back to `price` if no standard price is known). */
export const unitPriceFor = (product: PricedProduct, bulkUnlocked: boolean) =>
  bulkUnlocked ? product.price : (product.standardPrice ?? product.price);

export interface CartTotals {
  /** Amount payable: `listTotal` when the bulk tier is unlocked, else `standardTotal`. */
  total: number;
  mrpTotal: number;
  savings: number;
  units: number;
  /** Effective discount across the cart, rounded. */
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
  const total = bulkUnlocked ? listTotal : standardTotal;
  const mrpTotal = sumRupees(items.map((i) => lineTotal(i.product.originalPrice, i.quantity)));
  const savings = Math.max(0, sumRupees([mrpTotal, -total]));
  const units = items.reduce((acc, i) => acc + i.quantity, 0);
  const discountPercent = mrpTotal > 0 ? Math.round((savings / mrpTotal) * 100) : 0;
  const amountToUnlock =
    bulkUnlocked || listTotal <= 0 ? 0 : Math.max(0, sumRupees([rules.bulkMinOrder, -listTotal]));
  const unlockProgress =
    rules.bulkMinOrder > 0 ? Math.min(1, Math.max(0, listTotal / rules.bulkMinOrder)) : 1;
  const bulkSavings = Math.max(0, sumRupees([standardTotal, -listTotal]));
  return {
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
