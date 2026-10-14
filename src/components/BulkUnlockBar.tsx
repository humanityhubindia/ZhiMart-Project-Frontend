import React, { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { BadgePercent, ChevronRight, PartyPopper, X } from 'lucide-react';
import { CartTotals, formatRupees, hasBulkThreshold } from '../lib/money';
import { useTestMode } from '../hooks/useProducts';

interface BulkUnlockBarProps {
  totals: CartTotals;
  /** Opens the cart. */
  onOpenCart: () => void;
  /**
   * Hide the bar (e.g. on the cart page, which shows the full progress card). The component stays
   * mounted so tier changes are still tracked.
   */
  suppressed?: boolean;
}

const CELEBRATE_MS = 4500;

/**
 * Slim sticky hint shown while browsing with a non-empty cart below the bulk threshold:
 * "₹X more for 85% OFF" (click → cart). When the threshold is crossed it briefly celebrates and
 * then hides. Dismissible; the dismissal lasts until the cart is emptied. Never shown in test mode.
 *
 * Placement: bottom-left on phones (clear of the WhatsApp / scroll-to-top buttons at the bottom
 * right), centred on wider screens.
 */
export const BulkUnlockBar: React.FC<BulkUnlockBarProps> = ({ totals, onOpenCart, suppressed = false }) => {
  const testMode = useTestMode();
  const reduceMotion = useReducedMotion();
  const [dismissed, setDismissed] = useState(false);
  const [celebrating, setCelebrating] = useState(false);
  const prevUnlocked = useRef(totals.bulkUnlocked);

  const hasItems = totals.listTotal > 0;

  // Celebrate the moment the cart crosses the threshold; stop if it drops back below.
  useEffect(() => {
    const was = prevUnlocked.current;
    prevUnlocked.current = totals.bulkUnlocked;
    if (!was && totals.bulkUnlocked && hasItems) {
      setCelebrating(true);
      const id = window.setTimeout(() => setCelebrating(false), CELEBRATE_MS);
      return () => window.clearTimeout(id);
    }
    if (!totals.bulkUnlocked) setCelebrating(false);
    return undefined;
  }, [totals.bulkUnlocked, hasItems]);

  // An emptied cart starts a fresh shopping session: show the hint again next time.
  useEffect(() => {
    if (!hasItems) setDismissed(false);
  }, [hasItems]);

  if (testMode || !hasBulkThreshold(totals.rules)) return null;

  const { rules } = totals;
  const showLocked = hasItems && !totals.bulkUnlocked && !dismissed;
  const showCelebration = celebrating && totals.bulkUnlocked;
  const visible = !suppressed && (showLocked || showCelebration);
  const percent = Math.max(4, Math.round(totals.unlockProgress * 100));

  const motionProps = reduceMotion
    ? { initial: { opacity: 0 }, animate: { opacity: 1 }, exit: { opacity: 0 } }
    : {
        initial: { opacity: 0, y: 24 },
        animate: { opacity: 1, y: 0 },
        exit: { opacity: 0, y: 24, scale: 0.96 },
        transition: { type: 'spring' as const, stiffness: 420, damping: 32 },
      };

  return (
    <>
      {/* Keeps the end of the page (footer) scrollable out from under the bar. */}
      {visible && <div aria-hidden="true" className="h-20" />}

      <div className="fixed z-30 bottom-6 left-4 right-24 sm:right-auto sm:left-1/2 sm:-translate-x-1/2 sm:w-[26rem] pointer-events-none">
        <AnimatePresence>
          {visible && (
            <motion.div
              key={showCelebration ? 'unlocked' : 'locked'}
              {...motionProps}
              className={`pointer-events-auto flex items-center rounded-2xl border shadow-xl backdrop-blur-md ${
                showCelebration
                  ? 'bg-emerald-600/95 border-emerald-400 text-white'
                  : 'bg-slate-900/95 border-amber-400/40 text-white'
              }`}
            >
              {showCelebration ? (
                <button
                  type="button"
                  onClick={onOpenCart}
                  className="flex-1 min-w-0 flex items-center gap-2 px-3.5 py-3 text-left"
                >
                  <PartyPopper className="w-4 h-4 shrink-0 text-emerald-100" aria-hidden="true" />
                  <span role="status" aria-live="polite" className="text-xs font-extrabold leading-tight">
                    {rules.bulkDiscountPercent}% OFF unlocked! <span aria-hidden="true">🎉</span>
                    {totals.bulkSavings > 0 && (
                      <span className="font-semibold text-emerald-50">
                        {' '}
                        You save ₹{formatRupees(totals.bulkSavings)} extra
                      </span>
                    )}
                  </span>
                </button>
              ) : (
                <>
                  <button
                    type="button"
                    onClick={onOpenCart}
                    className="flex-1 min-w-0 px-3.5 py-2.5 text-left group"
                    aria-label={`Add ₹${formatRupees(totals.amountToUnlock)} more to unlock ${rules.bulkDiscountPercent}% OFF. Open cart.`}
                  >
                    <span className="flex items-center gap-2 text-xs font-extrabold leading-tight">
                      <BadgePercent className="w-4 h-4 shrink-0 text-amber-400" aria-hidden="true" />
                      <span role="status" aria-live="polite" className="truncate">
                        ₹{formatRupees(totals.amountToUnlock)} more for{' '}
                        <span className="text-amber-400">{rules.bulkDiscountPercent}% OFF</span>
                      </span>
                      <span className="ml-auto hidden min-[400px]:inline-flex items-center text-[10px] font-bold text-slate-300 group-hover:text-white shrink-0">
                        View cart
                        <ChevronRight className="w-3.5 h-3.5" aria-hidden="true" />
                      </span>
                    </span>
                    <span aria-hidden="true" className="mt-1.5 block h-1.5 w-full rounded-full bg-slate-700 overflow-hidden">
                      <span
                        className="block h-full rounded-full bg-gradient-to-r from-amber-400 to-orange-500 transition-[width] duration-500 ease-out motion-reduce:transition-none"
                        style={{ width: `${percent}%` }}
                      />
                    </span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setDismissed(true)}
                    className="self-stretch px-2.5 text-slate-400 hover:text-white border-l border-slate-700/80 rounded-r-2xl"
                    aria-label="Hide bulk discount hint"
                    title="Hide"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </>
  );
};
