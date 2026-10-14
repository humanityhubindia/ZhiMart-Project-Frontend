import React from 'react';
import { ArrowRight, BadgePercent, PartyPopper } from 'lucide-react';
import { cn } from '@/lib/utils';
import { CartTotals, formatRupees, hasBulkThreshold } from '../lib/money';
import { useTestMode } from '../hooks/useProducts';

interface BulkDiscountProgressProps {
  totals: CartTotals;
  /** `full` = cart page card, `compact` = one-line summary (drawer / order summary). */
  variant?: 'full' | 'compact';
  /** Optional "add more items" action shown while the bulk tier is locked. */
  onShopMore?: () => void;
  className?: string;
}

/** Visible text for the bulk-tier state; also used by the sticky bar so the copy stays in sync. */
export function bulkTierHeadline(totals: CartTotals): string {
  const { rules } = totals;
  if (!hasBulkThreshold(rules)) return `${rules.bulkDiscountPercent}% OFF on every order`;
  if (totals.listTotal <= 0) {
    return `Orders of ₹${formatRupees(rules.bulkMinOrder)}+ get ${rules.bulkDiscountPercent}% OFF; below that ${rules.standardDiscountPercent}% OFF`;
  }
  if (totals.bulkUnlocked) return `${rules.bulkDiscountPercent}% OFF unlocked`;
  return `Add ₹${formatRupees(totals.amountToUnlock)} more to unlock ${rules.bulkDiscountPercent}% OFF`;
}

/**
 * Two-tier pricing nudge: how far the cart (at price-list prices) is from the bulk threshold,
 * what unlocking would save, or a celebration once it is unlocked. Hidden in ₹1 test mode.
 */
export const BulkDiscountProgress: React.FC<BulkDiscountProgressProps> = ({
  totals,
  variant = 'full',
  onShopMore,
  className,
}) => {
  const testMode = useTestMode();
  // Hidden in test mode, and when there is no minimum (every order already gets the bulk price).
  if (testMode || !hasBulkThreshold(totals.rules)) return null;

  const { rules, listTotal, bulkUnlocked, bulkSavings } = totals;
  const empty = listTotal <= 0;
  const compact = variant === 'compact';
  const percent = Math.round(totals.unlockProgress * 100);

  // Empty cart: just explain the rule.
  if (empty) {
    return (
      <div
        role="status"
        aria-live="polite"
        className={cn(
          'flex items-start gap-2 rounded-2xl border border-amber-200 bg-amber-50/70 text-amber-900',
          compact ? 'px-3 py-2 text-[11px]' : 'p-3.5 text-xs',
          className
        )}
      >
        <BadgePercent className="w-4 h-4 text-amber-600 shrink-0 mt-px" aria-hidden="true" />
        <p className="font-semibold leading-snug">{bulkTierHeadline(totals)}.</p>
      </div>
    );
  }

  const bar = (
    <div
      role="progressbar"
      aria-label={`Progress towards ${rules.bulkDiscountPercent}% OFF`}
      aria-valuemin={0}
      aria-valuemax={rules.bulkMinOrder}
      aria-valuenow={Math.min(listTotal, rules.bulkMinOrder)}
      aria-valuetext={
        bulkUnlocked
          ? `${rules.bulkDiscountPercent}% OFF unlocked`
          : `₹${formatRupees(listTotal)} of ₹${formatRupees(rules.bulkMinOrder)}`
      }
      className={cn(
        'w-full overflow-hidden rounded-full',
        compact ? 'h-1.5' : 'h-2.5',
        bulkUnlocked ? 'bg-emerald-100' : 'bg-amber-100'
      )}
    >
      <div
        className={cn(
          'h-full rounded-full transition-[width] duration-500 ease-out motion-reduce:transition-none',
          bulkUnlocked
            ? 'bg-gradient-to-r from-emerald-400 to-emerald-600'
            : 'bg-gradient-to-r from-amber-400 to-orange-500'
        )}
        style={{ width: `${Math.max(percent, 3)}%` }}
      />
    </div>
  );

  if (bulkUnlocked) {
    return (
      <div
        className={cn(
          'rounded-2xl border border-emerald-200 bg-emerald-50/80 text-emerald-900',
          compact ? 'px-3 py-2 space-y-1.5' : 'p-4 space-y-2.5',
          className
        )}
      >
        <div role="status" aria-live="polite" className="flex items-start gap-2">
          <PartyPopper
            className={cn('text-emerald-600 shrink-0', compact ? 'w-4 h-4' : 'w-5 h-5')}
            aria-hidden="true"
          />
          <p className={cn('leading-snug', compact ? 'text-[11px]' : 'text-xs')}>
            <strong className={cn('font-black', compact ? 'text-xs' : 'text-sm')}>
              {rules.bulkDiscountPercent}% OFF unlocked <span aria-hidden="true">🎉</span>
            </strong>
            {bulkSavings > 0 && (
              <>
                {' '}
                <span className="font-semibold">
                  — you saved ₹{formatRupees(bulkSavings)} extra vs the {rules.standardDiscountPercent}% price.
                </span>
              </>
            )}
          </p>
        </div>
        {!compact && bar}
      </div>
    );
  }

  // Locked: below the threshold, every line is charged the standard price.
  return (
    <div
      className={cn(
        'rounded-2xl border border-amber-200 bg-gradient-to-br from-amber-50 to-orange-50/60 text-amber-950',
        compact ? 'px-3 py-2 space-y-1.5' : 'p-4 space-y-2.5',
        className
      )}
    >
      <div role="status" aria-live="polite" className="flex items-start gap-2">
        <BadgePercent
          className={cn('text-orange-500 shrink-0', compact ? 'w-4 h-4' : 'w-5 h-5')}
          aria-hidden="true"
        />
        <div className="space-y-0.5 min-w-0">
          <p className={cn('font-black leading-snug', compact ? 'text-xs' : 'text-sm')}>
            Add ₹{formatRupees(totals.amountToUnlock)} more to unlock {rules.bulkDiscountPercent}% OFF
          </p>
          {bulkSavings > 0 && (
            <p className={cn('font-semibold text-amber-800 leading-snug', compact ? 'text-[10px]' : 'text-[11px]')}>
              Unlocking saves ₹{formatRupees(bulkSavings)} on your current cart
              {compact ? '.' : ` — every item drops from its ${rules.standardDiscountPercent}% OFF price to ${rules.bulkDiscountPercent}% OFF.`}
            </p>
          )}
        </div>
      </div>

      {bar}

      {!compact && (
        <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] font-semibold text-amber-800">
          <span>
            ₹{formatRupees(listTotal)} of ₹{formatRupees(rules.bulkMinOrder)} at price-list rates &bull; paying{' '}
            {rules.standardDiscountPercent}% OFF prices now
          </span>
          {onShopMore && (
            <button
              type="button"
              onClick={onShopMore}
              className="inline-flex items-center gap-1 font-extrabold text-orange-700 hover:text-orange-800 underline-offset-2 hover:underline"
            >
              Add more items
              <ArrowRight className="w-3.5 h-3.5" aria-hidden="true" />
            </button>
          )}
        </div>
      )}
    </div>
  );
};
