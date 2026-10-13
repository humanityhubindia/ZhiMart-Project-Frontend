import React, { useState } from 'react';
import {
  ArrowLeft,
  Calculator,
  Plus,
  Minus,
  ShoppingBag,
  Check,
  Share2,
  Printer,
  RotateCcw,
  Package,
  MessageCircle,
  BadgeCheck,
} from 'lucide-react';
import { CrackerProduct } from '../types';
import { useProducts, countBy, useTestMode, usePricingRules } from '../hooks/useProducts';
import { CartTotals, computeTotals, formatRupees, hasBulkThreshold, lineTotal, PricingRules, unitPriceFor } from '../lib/money';
import { StockBadge } from './StockBadge';
import { CatalogErrorCard, EmptyCatalogState, ListSkeleton, errorMessage } from './CatalogStates';

/** Short tab labels for known categories; anything else falls back to Title Case. */
const CATEGORY_LABELS: Record<string, string> = {
  'SINGLE SOUND CRACKERS': 'Single Sound',
  'ELECTRIC TAPE FUSE CRACKERS': 'Electric Tape',
  'DELUXE WIRE FUSE CRACKERS': 'Deluxe Wire Fuse',
  'FLOWER POTS': 'Flower Pots',
  'GROUND CHAKKARS': 'Ground Chakkars',
  'AERIAL REPEATERS': 'Aerial Repeaters',
  'GIFT BOXES & HAMPERS': 'Gift Boxes',
};

export const categoryLabel = (category: string) =>
  CATEGORY_LABELS[category] ??
  category
    .toLowerCase()
    .replace(/\b\w/g, (c) => c.toUpperCase());

/** "85% OFF on orders ₹10,000+ • 70% OFF below" */
export const tierSubtitle = (rules: PricingRules) =>
  hasBulkThreshold(rules)
    ? `${rules.bulkDiscountPercent}% OFF on orders ₹${formatRupees(rules.bulkMinOrder)}+ • ${rules.standardDiscountPercent}% OFF below`
    : `${rules.bulkDiscountPercent}% OFF on every order`;

/** Rate cell: "₹32 / ₹63" with the tier that currently applies emphasised (single price when equal). */
export const TierRate: React.FC<{ product: CrackerProduct; bulkUnlocked: boolean; testMode: boolean }> = ({
  product,
  bulkUnlocked,
  testMode,
}) => {
  if (testMode || !(product.standardPrice > product.price)) {
    return <span className="font-extrabold text-slate-900">₹{formatRupees(product.price)}</span>;
  }
  const active = 'font-extrabold text-slate-900';
  const inactive = 'font-semibold text-slate-400';
  return (
    <span className="whitespace-nowrap" title={`₹${formatRupees(product.price)} on bulk orders · ₹${formatRupees(product.standardPrice)} below the bulk threshold`}>
      <span className={bulkUnlocked ? active : inactive}>₹{formatRupees(product.price)}</span>
      <span className="text-slate-300 font-normal"> / </span>
      <span className={bulkUnlocked ? inactive : active}>₹{formatRupees(product.standardPrice)}</span>
    </span>
  );
};

/**
 * One-line tier status for an estimate: applied, or how much more (at list prices) unlocks the
 * bulk price, with a small progress bar.
 */
export const BulkTierSummary: React.FC<{ totals: CartTotals; className?: string }> = ({ totals, className = '' }) => {
  const { rules } = totals;
  if (!hasBulkThreshold(rules)) return null;
  const threshold = `₹${formatRupees(rules.bulkMinOrder)}`;
  if (totals.listTotal <= 0) {
    return (
      <p className={`text-[11px] font-semibold text-slate-500 ${className}`}>
        Orders of {threshold}+ (at list price) get {rules.bulkDiscountPercent}% OFF; smaller orders get{' '}
        {rules.standardDiscountPercent}% OFF.
      </p>
    );
  }
  if (totals.bulkUnlocked) {
    return (
      <p className={`text-xs font-bold text-emerald-700 flex items-start gap-1.5 ${className}`}>
        <BadgeCheck className="w-4 h-4 shrink-0" aria-hidden="true" />
        <span>
          {rules.bulkDiscountPercent}% bulk price applied
          {totals.bulkSavings > 0 && ` — ₹${formatRupees(totals.bulkSavings)} less than the ${rules.standardDiscountPercent}% price`}
        </span>
      </p>
    );
  }
  const pct = Math.round(totals.unlockProgress * 100);
  return (
    <div className={`space-y-1.5 ${className}`}>
      <p className="text-xs font-semibold text-amber-900">
        Add <strong>₹{formatRupees(totals.amountToUnlock)}</strong> more (at list price) to unlock{' '}
        {rules.bulkDiscountPercent}% OFF
        {totals.bulkSavings > 0 && <> — you'd save <strong>₹{formatRupees(totals.bulkSavings)}</strong></>}
      </p>
      <div
        className="h-1.5 w-full rounded-full bg-amber-100 overflow-hidden"
        role="progressbar"
        aria-label={`Progress towards ${threshold}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
      >
        <div className="h-full rounded-full bg-amber-500 transition-all duration-300" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
};

interface QuickEstimatePageProps {
  onBack: () => void;
  onAddEstimateToCart: (items: { product: CrackerProduct; quantity: number }[]) => void;
}

export const QuickEstimatePage: React.FC<QuickEstimatePageProps> = ({
  onBack,
  onAddEstimateToCart,
}) => {
  const productsQuery = useProducts();
  const testMode = useTestMode();
  const rules = usePricingRules();
  const allProducts = productsQuery.data ?? [];

  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [selectedCategory, setSelectedCategory] = useState<string>('all');
  const [addedSuccess, setAddedSuccess] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);

  const categories = [
    { label: 'All Crackers', value: 'all' },
    ...countBy(allProducts, 'category').map((c) => ({ label: categoryLabel(c.name), value: c.name })),
  ];

  const filteredProducts = selectedCategory === 'all'
    ? allProducts
    : allProducts.filter((p) => p.category === selectedCategory);

  const maxFor = (product: CrackerProduct) => (product.inStock ? Math.max(0, product.availableQuantity) : 0);

  const handleSetQty = (product: CrackerProduct, value: number) => {
    const next = Math.max(0, Math.min(maxFor(product), Math.floor(value) || 0));
    setQuantities((prev) => ({ ...prev, [product.id]: next }));
  };

  const handleQtyChange = (product: CrackerProduct, delta: number) => {
    handleSetQty(product, (quantities[product.id] || 0) + delta);
  };

  const handleResetAll = () => {
    setQuantities({});
  };

  // Only orderable products, with quantities re-capped against the latest stock.
  const selectedItems = allProducts
    .map((product) => ({ product, quantity: Math.min(quantities[product.id] || 0, maxFor(product)) }))
    .filter((item) => item.quantity > 0);

  const totals = computeTotals(selectedItems, rules);
  const {
    total: totalEstimate,
    mrpTotal: totalOriginal,
    savings: totalSavings,
    units: totalPacksCount,
    discountPercent,
    bulkUnlocked,
  } = totals;
  const tierNote = testMode || selectedItems.length === 0
    ? ''
    : bulkUnlocked
      ? ` — ${rules.bulkDiscountPercent}% bulk price`
      : ` — ${rules.standardDiscountPercent}% price (add ₹${formatRupees(totals.amountToUnlock)} more for ${rules.bulkDiscountPercent}% OFF)`;

  const handleApplyToCart = () => {
    if (selectedItems.length > 0) {
      onAddEstimateToCart(selectedItems);
      setAddedSuccess(true);
      setTimeout(() => {
        setAddedSuccess(false);
      }, 2500);
    }
  };

  const handleShareEstimate = () => {
    const text = encodeURIComponent(
      `Zhi-mart Diwali Estimate: ${selectedItems.length} items, Total: ₹${formatRupees(totalEstimate)}${tierNote}${testMode ? '' : ` (Saved ₹${formatRupees(totalSavings)})`}!\n${window.location.href}`
    );
    window.open(`https://api.whatsapp.com/send?text=${text}`, '_blank');
  };

  const handleCopyEstimateLink = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(window.location.href);
      setCopiedLink(true);
      setTimeout(() => setCopiedLink(false), 2000);
    }
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8 animate-in fade-in duration-200 text-left">
      {/* 1. Header Navigation Bar */}
      <div className="flex flex-wrap items-center justify-between gap-4 pb-6 border-b border-slate-200">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-2 text-sm font-bold text-slate-700 hover:text-slate-900 bg-white border border-slate-200 px-4 py-2 rounded-xl transition-all shadow-2xs hover:bg-slate-50 active:scale-97"
        >
          <ArrowLeft className="w-4 h-4" />
          <span>Back to Home / Catalog</span>
        </button>

        <div className="flex items-center gap-2">
          <button
            onClick={handleResetAll}
            className="inline-flex items-center gap-1.5 text-xs font-semibold px-3.5 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-700 transition-all shadow-2xs"
          >
            <RotateCcw className="w-3.5 h-3.5" />
            <span>Reset All</span>
          </button>

          <button
            onClick={() => window.print()}
            className="inline-flex items-center gap-1.5 text-xs font-semibold px-3.5 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-700 transition-all shadow-2xs"
          >
            <Printer className="w-3.5 h-3.5" />
            <span>Print Sheet</span>
          </button>

          <button
            onClick={handleCopyEstimateLink}
            className="inline-flex items-center gap-1.5 text-xs font-semibold px-3.5 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-700 transition-all shadow-2xs"
          >
            {copiedLink ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Share2 className="w-3.5 h-3.5" />}
            <span>{copiedLink ? 'Link Copied!' : 'Copy Link'}</span>
          </button>

          <button
            onClick={handleShareEstimate}
            className="inline-flex items-center gap-1.5 text-xs font-bold px-4 py-2 rounded-xl bg-[#25D366] hover:bg-[#20ba5a] text-white transition-all shadow-2xs"
          >
            <MessageCircle className="w-4 h-4" />
            <span>WhatsApp</span>
          </button>
        </div>
      </div>

      {/* 2. Hero Title Section */}
      <div className="py-6 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="w-12 h-12 rounded-2xl bg-[#0F172A] text-amber-400 flex items-center justify-center shadow-md">
            <Calculator className="w-6 h-6" />
          </div>
          <div>
            <h1 className="text-2xl sm:text-3xl font-extrabold text-[#0F172A] tracking-tight">
              Zhi-mart Quick Estimate Sheet
            </h1>
            <p className="text-xs sm:text-sm text-slate-500 font-medium mt-0.5">
              Direct Sivakasi wholesale price calculator{testMode ? '' : ` • ${tierSubtitle(rules)}`}
            </p>
          </div>
        </div>

        <div className="bg-amber-50 border border-amber-200/80 px-4 py-2 rounded-2xl flex items-center gap-3 text-xs">
          <div>
            <span className="text-[10px] text-amber-700 uppercase font-black tracking-wide block">
              Estimated Total
            </span>
            <span className="text-lg font-black text-amber-950">
              ₹{formatRupees(totalEstimate)}
            </span>
          </div>
          <div className="h-8 w-px bg-amber-200" />
          <div>
            <span className="text-[10px] text-emerald-700 uppercase font-black tracking-wide block">
              {testMode ? 'Pricing' : `You Save (${discountPercent}%)`}
            </span>
            <span className="text-sm font-extrabold text-emerald-600">
              {testMode ? 'Test ₹1 / unit' : `₹${formatRupees(totalSavings)}`}
            </span>
          </div>
        </div>
      </div>

      {/* 3. Category Filter Tabs */}
      <div className="flex items-center gap-2 overflow-x-auto pb-4 no-scrollbar">
        {categories.map((c) => (
          <button
            key={c.value}
            onClick={() => setSelectedCategory(c.value)}
            className={`px-4 py-2 rounded-xl text-xs font-bold transition-all whitespace-nowrap ${
              selectedCategory === c.value
                ? 'bg-[#0F172A] text-white shadow-xs'
                : 'bg-white border border-slate-200 text-slate-700 hover:bg-slate-50'
            }`}
          >
            {c.label}
          </button>
        ))}
      </div>

      {/* 4. Full Estimation Table matching user's screenshot */}
      <div className="bg-white rounded-3xl border border-slate-200/90 shadow-sm overflow-hidden mt-2">
        {productsQuery.isPending ? (
          <ListSkeleton rows={8} />
        ) : productsQuery.isError && allProducts.length === 0 ? (
          <CatalogErrorCard
            className="m-6"
            message={errorMessage(productsQuery.error)}
            onRetry={() => void productsQuery.refetch()}
            isRetrying={productsQuery.isFetching}
          />
        ) : filteredProducts.length === 0 ? (
          <EmptyCatalogState className="m-6" message="No products available in this category right now." />
        ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-slate-200/80 bg-slate-50/70 text-slate-500 font-bold uppercase text-[11px] tracking-wider">
                <th className="py-4 pl-6">PRODUCT NAME</th>
                <th className="py-4 px-4">PACK DETAILS</th>
                <th className="py-4 px-4 text-right whitespace-nowrap">
                  RATE
                  {!testMode && hasBulkThreshold(rules) && (
                    <span className="block text-[9px] font-semibold normal-case tracking-normal text-slate-400">
                      ₹{formatRupees(rules.bulkMinOrder)}+ / below
                    </span>
                  )}
                </th>
                <th className="py-4 px-4 text-center min-w-[140px]">QUANTITY</th>
                <th className="py-4 pr-6 text-right">TOTAL (₹)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {filteredProducts.map((prod) => {
                const max = maxFor(prod);
                const isOutOfStock = max <= 0;
                const qty = Math.min(quantities[prod.id] || 0, max);
                const rowTotal = lineTotal(unitPriceFor(prod, bulkUnlocked), qty);

                return (
                  <tr
                    key={prod.id}
                    className={`transition-colors hover:bg-slate-50/80 ${
                      qty > 0 ? 'bg-amber-50/20' : ''
                    } ${isOutOfStock ? 'bg-slate-50/60' : ''}`}
                  >
                    {/* Product Name */}
                    <td className="py-4 pl-6">
                      <div className="flex items-center gap-3">
                        <img
                          src={prod.image}
                          alt={prod.name}
                          className={`w-10 h-10 object-contain rounded-lg border border-slate-100 p-0.5 bg-white shrink-0 ${
                            isOutOfStock ? 'opacity-50 grayscale' : ''
                          }`}
                        />
                        <div>
                          <div className="font-bold text-slate-900 text-sm leading-snug">
                            {prod.name}
                          </div>
                          <div className="text-[10px] font-bold text-slate-400 uppercase tracking-wider mt-0.5">
                            {prod.category}
                          </div>
                          <StockBadge product={prod} className="mt-1 text-[10px] px-2 py-0.5" />
                        </div>
                      </div>
                    </td>

                    {/* Pack Details */}
                    <td className="py-4 px-4 font-semibold text-slate-600 text-xs">
                      <span className="inline-flex items-center gap-1.5 bg-slate-100 px-2.5 py-1 rounded-md">
                        <Package className="w-3 h-3 text-amber-700" />
                        <span>{prod.packInfo}</span>
                      </span>
                    </td>

                    {/* Rate */}
                    <td className="py-4 px-4 text-right">
                      <div className="text-sm">
                        <TierRate product={prod} bulkUnlocked={bulkUnlocked} testMode={testMode} />
                      </div>
                      {!testMode && prod.originalPrice > prod.price && (
                        <div className="text-xs text-slate-400 line-through">
                          ₹{formatRupees(prod.originalPrice)}
                        </div>
                      )}
                    </td>

                    {/* Quantity Stepper */}
                    <td className="py-4 px-4">
                      {isOutOfStock ? (
                        <div className="text-center text-[11px] font-bold text-slate-400">Unavailable</div>
                      ) : (
                        <div className="flex items-center justify-center space-x-2 bg-slate-100 rounded-xl p-1 max-w-[120px] mx-auto">
                          <button
                            type="button"
                            onClick={() => handleQtyChange(prod, -1)}
                            disabled={qty <= 0}
                            className="p-1 hover:bg-white rounded-lg text-slate-700 active:scale-90 transition-all disabled:opacity-30"
                            aria-label="Decrease"
                          >
                            <Minus className="w-3.5 h-3.5" />
                          </button>
                          <input
                            type="number"
                            min="0"
                            max={max}
                            value={qty}
                            onChange={(e) => handleSetQty(prod, parseInt(e.target.value, 10) || 0)}
                            className="w-10 text-center font-bold text-xs bg-transparent outline-none"
                            aria-label={`Quantity for ${prod.name}`}
                          />
                          <button
                            type="button"
                            onClick={() => handleQtyChange(prod, 1)}
                            disabled={qty >= max}
                            title={qty >= max ? 'No more stock available' : undefined}
                            className="p-1 hover:bg-white rounded-lg text-slate-700 active:scale-90 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                            aria-label="Increase"
                          >
                            <Plus className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      )}
                    </td>

                    {/* Total (₹) */}
                    <td className="py-4 pr-6 text-right font-black text-slate-900 text-sm">
                      {rowTotal > 0 ? (
                        <span className="text-slate-900">₹{formatRupees(rowTotal)}</span>
                      ) : (
                        <span className="text-slate-300">—</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        )}

        {/* 5. Sticky Bottom Summary Bar matching user's screenshot */}
        <div className="p-6 bg-white border-t border-slate-200/90 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="text-left space-y-1 w-full sm:w-auto sm:max-w-xl">
            <div className="text-sm font-semibold text-slate-700">
              Total Estimate Value:{' '}
              <span className="font-black text-lg text-slate-900">₹{formatRupees(totalEstimate)}</span>{' '}
              <span className="text-xs text-slate-400 line-through font-normal">
                (₹{formatRupees(totalOriginal)})
              </span>
            </div>
            <div className="text-xs font-bold text-emerald-600">
              {testMode
                ? 'Test mode — every unit is charged ₹1'
                : `You Save: ₹${formatRupees(totalSavings)} (${discountPercent}% Festival Savings)`}{' '}
              &bull; {totalPacksCount} packs
            </div>
            {!testMode && <BulkTierSummary totals={totals} className="pt-1" />}
          </div>

          <div className="flex items-center gap-3 w-full sm:w-auto">
            <button
              onClick={handleApplyToCart}
              disabled={selectedItems.length === 0}
              className="w-full sm:w-auto bg-[#0F172A] hover:bg-slate-800 disabled:opacity-40 text-white font-bold px-8 py-3.5 rounded-2xl transition-all shadow-md flex items-center justify-center gap-2 active:scale-97 text-sm"
            >
              {addedSuccess ? (
                <>
                  <Check className="w-4 h-4 text-emerald-400" />
                  <span>All Items Added to Bag!</span>
                </>
              ) : (
                <>
                  <ShoppingBag className="w-4 h-4 text-amber-400" />
                  <span>Add All Items to Cart</span>
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
