import React, { useState } from 'react';
import {
  ArrowLeft,
  Bot,
  Check,
  ShoppingBag,
  Share2,
  RefreshCw,
  ShieldCheck,
  Volume2,
  Users,
  Flame,
  MessageCircle,
  Sliders,
  DollarSign
} from 'lucide-react';
import { CrackerProduct } from '../types';
import { useProducts, inStockOnly, useTestMode, usePricingRules } from '../hooks/useProducts';
import { CartTotals, computeTotals, formatRupees, hasBulkThreshold, lineTotal, PricingRules, sumRupees, unitPriceFor } from '../lib/money';
import { openWhatsApp } from '../lib/checkout';
import { StockBadge } from './StockBadge';
import { CatalogErrorCard, ListSkeleton, errorMessage } from './CatalogStates';

type PlanLine = { product: CrackerProduct; quantity: number };
type PriceOf = (p: CrackerProduct) => number;

/**
 * Plans a combo for a budget under two-tier pricing. `build(priceOf, minListTotal)` picks lines
 * using the given unit price (and may top up quantities until `minListTotal` is reached).
 *  - ₹1 test mode: planned by MRP so recommendations stay realistic.
 *  - Budget ≥ `bulkMinOrder`: planned at the price-list (bulk) price, kept only if the plan's list
 *    total actually reaches the threshold — otherwise the standard price would apply and the plan
 *    would overshoot the budget, so it is re-planned at the standard price.
 *  - Budget below the threshold: planned at the standard price.
 * The plan's real cost is always `computeTotals(items, rules).total`.
 */
export function planForBudget(
  budget: number,
  rules: PricingRules,
  testMode: boolean,
  build: (priceOf: PriceOf, minListTotal: number) => PlanLine[]
): PlanLine[] {
  if (testMode) return build((p) => p.originalPrice, 0);
  if (budget >= rules.bulkMinOrder) {
    const bulkPlan = build((p) => p.price, rules.bulkMinOrder);
    if (computeTotals(bulkPlan, rules).bulkUnlocked) return bulkPlan;
  }
  return build((p) => unitPriceFor(p, false), 0);
}

/** One-line explanation of which price tier a plan is charged at. */
export function planTierNote(totals: CartTotals, budget: number): string {
  const { rules } = totals;
  const threshold = `₹${formatRupees(rules.bulkMinOrder)}`;
  if (totals.units === 0 || !hasBulkThreshold(rules)) return '';
  if (totals.bulkUnlocked) {
    return `${rules.bulkDiscountPercent}% bulk price applies — this plan is over ${threshold} at list prices.`;
  }
  if (budget >= rules.bulkMinOrder) {
    return `Not enough matching crackers reach ${threshold} at list prices, so the ${rules.standardDiscountPercent}% OFF price applies.`;
  }
  return `Priced at ${rules.standardDiscountPercent}% OFF (orders below ${threshold}). Budget ${threshold}+ to get ${rules.bulkDiscountPercent}% OFF on every item.`;
}

interface AIAssistantPageProps {
  onBack: () => void;
  onAddComboToCart: (items: { product: CrackerProduct; quantity: number }[]) => void;
  onNavigateToCart?: () => void;
  onNavigateToProduct?: (product: CrackerProduct) => void;
}

export const AIAssistantPage: React.FC<AIAssistantPageProps> = ({
  onBack,
  onAddComboToCart,
  onNavigateToCart,
  onNavigateToProduct,
}) => {
  // Config state
  const [budget, setBudget] = useState<number>(2500);
  const [customBudgetInput, setCustomBudgetInput] = useState('');
  const [occasion, setOccasion] = useState<'family' | 'kids' | 'party' | 'temple'>('family');
  const [soundPref, setSoundPref] = useState<'balanced' | 'silent' | 'loud'>('balanced');
  const [includeAerials, setIncludeAerials] = useState(true);
  const [includeGiftBox, setIncludeGiftBox] = useState(false);
  const [isGenerated, setIsGenerated] = useState(true);
  const [addedSuccess, setAddedSuccess] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);

  const productsQuery = useProducts();
  const testMode = useTestMode();
  const rules = usePricingRules();
  // Never recommend products that cannot be ordered right now.
  const orderable = inStockOnly(productsQuery.data ?? []);

  // Dynamic AI curation logic based on budget, occasion, and sound preferences
  const generateCombo = (planningPrice: PriceOf, minListTotal: number) => {
    let pool = [...orderable];

    if (occasion === 'kids' || soundPref === 'silent') {
      pool = pool.filter(
        (p) =>
          p.category.includes('FLOWER') ||
          p.category.includes('GROUND') ||
          p.soundLevel === 'Silent & Visual' ||
          p.soundLevel === 'Low Sound'
      );
      if (pool.length < 3) {
        pool = orderable.filter((p) => !p.category.includes('SINGLE SOUND'));
      }
    } else if (occasion === 'party' || soundPref === 'loud') {
      pool = pool.filter(
        (p) =>
          p.category.includes('AERIAL') ||
          p.category.includes('SINGLE SOUND') ||
          p.category.includes('FUSE')
      );
      if (pool.length < 3) pool = [...orderable];
    }

    // Allocate items to match target budget as closely as possible
    const sumList = (lines: PlanLine[]) => computeTotals(lines, rules).listTotal;
    const cap = budget * 1.05;
    let currentTotal = 0;
    const fits = (amount: number) => sumRupees([currentTotal, amount]) <= cap;
    const selected: PlanLine[] = [];

    for (const prod of pool) {
      const unit = planningPrice(prod);
      if (fits(unit)) {
        let qty = 1;
        if (unit < 300 && budget >= 2500 && fits(lineTotal(unit, 2))) {
          qty = 2;
        }
        qty = Math.min(qty, prod.availableQuantity);
        if (qty <= 0) continue;
        selected.push({ product: prod, quantity: qty });
        currentTotal = sumRupees([currentTotal, lineTotal(unit, qty)]);
      }
    }

    // Bulk plans must reach the threshold at list prices: top up quantities round-robin while the
    // budget (and stock) allows.
    let listTotal = sumList(selected);
    for (let progress = true; listTotal < minListTotal && progress; ) {
      progress = false;
      for (const line of selected) {
        if (listTotal >= minListTotal) break;
        const unit = planningPrice(line.product);
        if (line.quantity < line.product.availableQuantity && fits(unit)) {
          line.quantity += 1;
          currentTotal = sumRupees([currentTotal, unit]);
          listTotal = sumList(selected);
          progress = true;
        }
      }
    }

    // Fallback if nothing fit the budget: the cheapest orderable product.
    if (selected.length === 0 && orderable.length > 0) {
      const cheapest = orderable.reduce(
        (min, p) => (planningPrice(p) < planningPrice(min) ? p : min),
        orderable[0]
      );
      selected.push({ product: cheapest, quantity: 1 });
    }

    return selected;
  };

  const comboItems = planForBudget(budget, rules, testMode, generateCombo);
  const comboTotals = computeTotals(comboItems, rules);
  const {
    total: comboTotal,
    mrpTotal: comboOriginal,
    savings: comboSavings,
    units: totalItemCount,
    discountPercent: comboDiscount,
    bulkUnlocked: comboBulk,
  } = comboTotals;
  const tierNote = testMode ? '' : planTierNote(comboTotals, budget);
  const unitOf = (p: CrackerProduct) => unitPriceFor(p, comboBulk);

  const handleApplyCombo = () => {
    if (comboItems.length > 0) {
      onAddComboToCart(comboItems);
      setAddedSuccess(true);
      setTimeout(() => {
        setAddedSuccess(false);
        if (onNavigateToCart) onNavigateToCart();
      }, 1200);
    }
  };

  const handleShareCombo = () => {
    if (comboItems.length === 0) return;
    const itemsList = comboItems
      .map((i) => `• ${i.product.name} x ${i.quantity} = ₹${formatRupees(lineTotal(unitOf(i.product), i.quantity))}`)
      .join('\n');
    openWhatsApp(
      `Zhi-mart AI Cracker Combo (Budget: ₹${formatRupees(budget)}):\n${itemsList}\nTotal: ₹${formatRupees(comboTotal)} ${testMode ? ' (test prices)' : ` (Saved ₹${formatRupees(comboSavings)})`}!${tierNote ? `\n${tierNote}` : ''}\n${window.location.href}`
    );
  };

  const handleCopyLink = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(window.location.href);
      setCopiedLink(true);
      setTimeout(() => setCopiedLink(false), 2000);
    }
  };

  // Preset Combo Quick Loaders
  const loadPreset = (presetBudget: number, presetOccasion: 'family' | 'kids' | 'party') => {
    setBudget(presetBudget);
    setOccasion(presetOccasion);
    setSoundPref(presetOccasion === 'kids' ? 'silent' : presetOccasion === 'party' ? 'loud' : 'balanced');
    setIsGenerated(true);
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8 animate-in fade-in duration-200 text-left">
      {/* 1. Top Header Navigation */}
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
            onClick={handleCopyLink}
            className="inline-flex items-center gap-1.5 text-xs font-semibold px-3.5 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-700 transition-all shadow-2xs"
          >
            {copiedLink ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Share2 className="w-3.5 h-3.5" />}
            <span>{copiedLink ? 'Link Copied!' : 'Share Plan'}</span>
          </button>

          <button
            onClick={handleShareCombo}
            className="inline-flex items-center gap-1.5 text-xs font-bold px-4 py-2 rounded-xl bg-[#25D366] hover:bg-[#20ba5a] text-white transition-all shadow-2xs"
          >
            <MessageCircle className="w-4 h-4" />
            <span>WhatsApp Plan</span>
          </button>
        </div>
      </div>

      {/* 2. Hero Banner */}
      <div className="py-6 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-2xl bg-[#0F172A] text-amber-400 flex items-center justify-center shadow-md">
            <Bot className="w-7 h-7" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-[10px] font-extrabold uppercase tracking-wider bg-amber-100 text-amber-900 px-2.5 py-0.5 rounded-full">
                AI Curator 2.0
              </span>
              <span className="text-[10px] text-slate-400 font-semibold">
                Sivakasi Factory Intelligent Engine
              </span>
            </div>
            <h1 className="text-2xl sm:text-3xl font-black text-slate-900 font-heading tracking-tight mt-1">
              Smart AI Cracker Assistant
            </h1>
            <p className="text-xs sm:text-sm text-slate-500 font-medium">
              Curate the ultimate festive crackers combo tailored specifically to your family size, budget, and sound preference.
            </p>
          </div>
        </div>

        {/* Quick presets row */}
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => loadPreset(1000, 'kids')}
            className="text-xs font-bold bg-white border border-slate-200 hover:border-slate-400 px-3 py-1.5 rounded-xl transition-all shadow-2xs text-slate-700"
          >
            ₹1,000 Kids Safe
          </button>
          <button
            type="button"
            onClick={() => loadPreset(2500, 'family')}
            className="text-xs font-bold bg-white border border-slate-200 hover:border-slate-400 px-3 py-1.5 rounded-xl transition-all shadow-2xs text-slate-700"
          >
            ₹2,500 Family Hit
          </button>
          <button
            type="button"
            onClick={() => loadPreset(5000, 'party')}
            className="text-xs font-bold bg-white border border-slate-200 hover:border-slate-400 px-3 py-1.5 rounded-xl transition-all shadow-2xs text-slate-700"
          >
            ₹5,000 Grand Show
          </button>
        </div>
      </div>

      {/* 3. Main Workspace: 2-Column Responsive Layout */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
        {/* Left Column: AI Tuning Parameters */}
        <div className="lg:col-span-5 bg-white border border-slate-200 rounded-3xl p-6 shadow-xs space-y-6">
          <div className="flex items-center justify-between border-b border-slate-100 pb-3">
            <h2 className="text-sm font-black text-slate-900 uppercase tracking-wide flex items-center gap-2">
              <Sliders className="w-4 h-4 text-amber-500" />
              <span>Celebration Parameters</span>
            </h2>
            <button
              onClick={() => {
                setBudget(2500);
                setOccasion('family');
                setSoundPref('balanced');
              }}
              className="text-[11px] font-semibold text-slate-400 hover:text-slate-700 flex items-center gap-1"
            >
              <RefreshCw className="w-3 h-3" />
              <span>Reset</span>
            </button>
          </div>

          {/* 1. Target Budget */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <label className="text-xs font-bold text-slate-800">
                Target Budget
              </label>
              <span className="text-sm font-black text-slate-900 bg-amber-50 px-3 py-0.5 rounded-lg border border-amber-200">
                ₹{formatRupees(budget)}
              </span>
            </div>

            {/* Budget Presets */}
            <div className="grid grid-cols-4 gap-2">
              {[1000, 2500, 5000, rules.bulkMinOrder > 5000 ? rules.bulkMinOrder : 10000].map((b) => (
                <button
                  key={b}
                  type="button"
                  onClick={() => setBudget(b)}
                  className={`py-2 text-xs font-extrabold rounded-xl border transition-all ${
                    budget === b
                      ? 'bg-[#0F172A] text-white border-[#0F172A] shadow-xs'
                      : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50'
                  }`}
                >
                  ₹{formatRupees(b)}
                </button>
              ))}
            </div>

            {/* Range Slider */}
            <input
              type="range"
              min="500"
              max={Math.max(15000, rules.bulkMinOrder + 5000)}
              step="500"
              value={budget}
              onChange={(e) => setBudget(Number(e.target.value))}
              className="w-full accent-amber-500 cursor-pointer"
            />
          </div>

          {/* 2. Celebration Occasion */}
          <div className="space-y-2.5">
            <label className="text-xs font-bold text-slate-800 block">
              Who is Celebrating?
            </label>
            <div className="grid grid-cols-2 gap-2.5">
              {[
                { id: 'family' as const, label: 'Family Diwali', desc: 'Balanced visual & sound mix' },
                { id: 'kids' as const, label: 'Kids Friendly', desc: 'Pots, sparklers & low sound' },
                { id: 'party' as const, label: 'Loud Party', desc: 'Aerial repeaters & thunder' },
                { id: 'temple' as const, label: 'Temple / Function', desc: 'Tape fuse garland rolls' },
              ].map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setOccasion(item.id)}
                  className={`p-3 text-left rounded-2xl border transition-all ${
                    occasion === item.id
                      ? 'bg-slate-900 text-white border-slate-900 shadow-xs'
                      : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50'
                  }`}
                >
                  <p className="text-xs font-bold">{item.label}</p>
                  <p
                    className={`text-[10px] mt-0.5 leading-tight ${
                      occasion === item.id ? 'text-slate-300' : 'text-slate-400'
                    }`}
                  >
                    {item.desc}
                  </p>
                </button>
              ))}
            </div>
          </div>

          {/* 3. Sound & Smoke Preference */}
          <div className="space-y-2.5">
            <label className="text-xs font-bold text-slate-800 block">
              Sound & Smoke Preference
            </label>
            <div className="grid grid-cols-3 gap-2">
              {[
                { id: 'silent' as const, label: 'Eco / Low Sound' },
                { id: 'balanced' as const, label: 'Balanced (50/50)' },
                { id: 'loud' as const, label: 'Heavy Thunder' },
              ].map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setSoundPref(item.id)}
                  className={`py-2 px-1 text-center text-xs font-bold rounded-xl border transition-all ${
                    soundPref === item.id
                      ? 'bg-[#0F172A] text-white border-[#0F172A]'
                      : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50'
                  }`}
                >
                  {item.label}
                </button>
              ))}
            </div>
          </div>

          {/* 4. Priorities Checkboxes */}
          <div className="space-y-2 pt-2 border-t border-slate-100">
            <label className="text-xs font-bold text-slate-800 block">
              Special Inclusions
            </label>
            <div className="space-y-2 text-xs">
              <label className="flex items-center gap-2.5 cursor-pointer text-slate-700">
                <input
                  type="checkbox"
                  checked={includeAerials}
                  onChange={(e) => setIncludeAerials(e.target.checked)}
                  className="rounded border-slate-300 accent-amber-500 w-4 h-4"
                />
                <span className="font-semibold">Include Multishot Aerial Repeaters</span>
              </label>

              <label className="flex items-center gap-2.5 cursor-pointer text-slate-700">
                <input
                  type="checkbox"
                  checked={includeGiftBox}
                  onChange={(e) => setIncludeGiftBox(e.target.checked)}
                  className="rounded border-slate-300 accent-amber-500 w-4 h-4"
                />
                <span className="font-semibold">Include Family Festival Hamper Box</span>
              </label>
            </div>
          </div>

          {/* Action: Generate button */}
          <button
            type="button"
            onClick={() => setIsGenerated(true)}
            className="w-full bg-[#0F172A] hover:bg-slate-800 active:scale-98 text-white font-extrabold py-3.5 rounded-2xl text-xs transition-all shadow-md flex items-center justify-center gap-2"
          >
            <Bot className="w-4 h-4 text-amber-400" />
            <span>Generate Best Cracker Combo</span>
          </button>
        </div>

        {/* Right Column: AI Generated Package Output */}
        <div className="lg:col-span-7 space-y-5">
          {/* Output Card */}
          <div className="bg-white border border-slate-200 rounded-3xl p-6 shadow-sm space-y-6">
            {/* Header info */}
            <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 border-b border-slate-100 pb-4">
              <div>
                <span className="text-[10px] font-extrabold uppercase tracking-wider text-emerald-700 bg-emerald-50 px-2.5 py-0.5 rounded-full border border-emerald-200">
                  AI Recommendation Ready
                </span>
                <h3 className="text-lg font-black text-slate-900 font-heading mt-1">
                  {occasion === 'kids'
                    ? 'Kids Safe Sparkler & Fountain Combo'
                    : occasion === 'party'
                    ? 'Mega Aerial Thunder & Sound Combo'
                    : 'Grand Family Diwali Celebration Combo'}
                </h3>
                <p className="text-xs text-slate-500 font-medium mt-0.5">
                  {totalItemCount} total packets curated &bull; Estimated ~40 mins of fireworks fun
                </p>
              </div>

              <div className="text-right sm:text-right">
                <span className="text-2xl font-black text-slate-900 block">
                  ₹{formatRupees(comboTotal)}
                </span>
                {testMode ? (
                  <span className="text-xs font-bold text-amber-700">Test prices (₹1 / unit)</span>
                ) : (
                  <span className="text-xs text-slate-400 line-through">
                    MRP: ₹{formatRupees(comboOriginal)}
                  </span>
                )}
              </div>
            </div>

            {/* Savings Banner */}
            <div className="bg-emerald-50 border border-emerald-200 rounded-2xl p-3.5 flex items-center justify-between text-xs text-emerald-800">
              <div className="flex items-center gap-2">
                <ShieldCheck className="w-4 h-4 text-emerald-600 shrink-0" />
                <span className="font-semibold">
                  {testMode ? (
                    'Test mode — every unit is charged ₹1.'
                  ) : (
                    <>
                      {comboDiscount}% Festive Discount Applied! You save <strong>₹{formatRupees(comboSavings)}</strong>.
                    </>
                  )}
                </span>
              </div>
              <span className="font-extrabold text-emerald-700">
                100% Sivakasi Genuine
              </span>
            </div>

            {tierNote && (
              <p className={`-mt-3 text-[11px] font-semibold ${comboBulk ? 'text-emerald-700' : 'text-slate-500'}`}>
                {tierNote}
              </p>
            )}

            {/* List of Curated Items */}
            <div className="space-y-3">
              <h4 className="text-xs font-bold text-slate-700 uppercase tracking-wider">
                Included Fireworks in this Combo
              </h4>

              {productsQuery.isPending ? (
                <ListSkeleton rows={4} className="border border-slate-200 rounded-2xl" />
              ) : productsQuery.isError && !productsQuery.data ? (
                <CatalogErrorCard
                  message={errorMessage(productsQuery.error)}
                  onRetry={() => void productsQuery.refetch()}
                  isRetrying={productsQuery.isFetching}
                />
              ) : comboItems.length === 0 ? (
                <div className="p-6 text-center text-xs font-semibold text-slate-500 border border-slate-200 rounded-2xl">
                  No products are in stock right now. Please check back soon.
                </div>
              ) : (
              <div className="divide-y divide-slate-100 border border-slate-200 rounded-2xl overflow-hidden bg-slate-50/50">
                {comboItems.map((item) => (
                  <div
                    key={item.product.id}
                    className="p-3.5 flex items-center justify-between gap-3 bg-white hover:bg-slate-50/80 transition-colors"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      <div
                        onClick={() => onNavigateToProduct && onNavigateToProduct(item.product)}
                        className="w-12 h-12 bg-white rounded-xl p-1 border border-slate-200 flex items-center justify-center shrink-0 cursor-pointer"
                      >
                        <img
                          src={item.product.image}
                          alt={item.product.name}
                          className="max-h-full max-w-full object-contain"
                        />
                      </div>
                      <div className="min-w-0">
                        <button
                          onClick={() => onNavigateToProduct && onNavigateToProduct(item.product)}
                          className="text-xs font-bold text-slate-900 hover:text-amber-600 transition-colors truncate block text-left"
                        >
                          {item.product.name}
                        </button>
                        <p className="text-[10px] text-slate-400 font-semibold uppercase">
                          {item.product.category} &bull; {item.product.packInfo}
                        </p>
                        <StockBadge product={item.product} className="mt-1 text-[10px] px-2 py-0.5" />
                      </div>
                    </div>

                    <div className="text-right shrink-0">
                      <span className="text-xs font-extrabold text-slate-900 block">
                        ₹{formatRupees(lineTotal(unitOf(item.product), item.quantity))}
                      </span>
                      <span className="text-[10px] text-slate-400">
                        Qty: {item.quantity} &times; ₹{formatRupees(unitOf(item.product))}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
              )}
            </div>

            {/* Add to Cart Actions */}
            <div className="pt-2 flex flex-col sm:flex-row items-center gap-3">
              <button
                type="button"
                onClick={handleApplyCombo}
                disabled={comboItems.length === 0}
                className="w-full sm:flex-1 bg-[#F59E0B] hover:bg-[#D97706] active:scale-98 text-[#0F172A] font-extrabold py-3.5 rounded-2xl text-xs transition-all shadow-md flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {addedSuccess ? (
                  <>
                    <Check className="w-4 h-4 text-emerald-800" />
                    <span>Added Combo to Bag!</span>
                  </>
                ) : (
                  <>
                    <ShoppingBag className="w-4 h-4" />
                    <span>Add Entire Combo to Cart (₹{formatRupees(comboTotal)})</span>
                  </>
                )}
              </button>

              <button
                type="button"
                onClick={handleShareCombo}
                disabled={comboItems.length === 0}
                className="w-full sm:w-auto disabled:opacity-50 bg-white hover:bg-slate-50 border border-slate-300 py-3.5 px-5 rounded-2xl text-xs font-bold text-slate-700 transition-all flex items-center justify-center gap-2 shadow-2xs"
              >
                <MessageCircle className="w-4 h-4 text-[#25D366]" />
                <span>Share Combo on WhatsApp</span>
              </button>
            </div>
          </div>

          {/* Trust Guarantees */}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
              <ShieldCheck className="w-5 h-5 text-emerald-600 shrink-0" />
              <div>
                <p className="text-xs font-bold text-slate-800">PESO Certified</p>
                <p className="text-[10px] text-slate-500">Government licensed</p>
              </div>
            </div>

            <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
              <Volume2 className="w-5 h-5 text-blue-600 shrink-0" />
              <div>
                <p className="text-xs font-bold text-slate-800">Decibel Verified</p>
                <p className="text-[10px] text-slate-500">Safe acoustic testing</p>
              </div>
            </div>

            <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
              <Flame className="w-5 h-5 text-amber-500 shrink-0" />
              <div>
                <p className="text-xs font-bold text-slate-800">Fresh Factory Stock</p>
                <p className="text-[10px] text-slate-500">2025 new production</p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
