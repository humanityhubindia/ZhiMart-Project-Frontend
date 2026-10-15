import { formatRupees, hasBulkThreshold, PricingRules } from './money';

/**
 * Human-readable line for the price tier an order was charged at, for invoices / receipts:
 * "Bulk price — 85% OFF (order ₹5,000+)" or "Standard price — 80% OFF (order below ₹5,000)".
 * Percentages and the threshold come from the current pricing rules.
 */
export function pricingTierLabel(tier: 'bulk' | 'standard', rules: PricingRules): string {
  const threshold = `₹${formatRupees(rules.bulkMinOrder)}`;
  if (tier === 'bulk') {
    return hasBulkThreshold(rules)
      ? `Bulk price — ${rules.bulkDiscountPercent}% OFF (order ${threshold}+)`
      : `Bulk price — ${rules.bulkDiscountPercent}% OFF`;
  }
  return `Standard price — ${rules.standardDiscountPercent}% OFF (order below ${threshold})`;
}
