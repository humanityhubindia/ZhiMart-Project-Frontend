import React, { useState } from 'react';
import { X, Calculator, Plus, Minus, Check, ShoppingBag } from 'lucide-react';
import { CrackerProduct } from '../types';
import { useProducts, useTestMode, usePricingRules } from '../hooks/useProducts';
import { computeTotals, formatRupees, hasBulkThreshold, lineTotal, unitPriceFor } from '../lib/money';
import { BulkTierSummary, deliveryNote, TierRate, tierSubtitle } from './QuickEstimatePage';
import { StockBadge } from './StockBadge';
import { CatalogErrorCard, EmptyCatalogState, ListSkeleton, errorMessage } from './CatalogStates';

interface QuickEstimateModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAddEstimateToCart: (items: { product: CrackerProduct; quantity: number }[]) => void;
}

export const QuickEstimateModal: React.FC<QuickEstimateModalProps> = ({
  isOpen,
  onClose,
  onAddEstimateToCart,
}) => {
  const productsQuery = useProducts();
  const testMode = useTestMode();
  const rules = usePricingRules();
  const allProducts = productsQuery.data ?? [];
  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [addedSuccess, setAddedSuccess] = useState(false);

  if (!isOpen) return null;

  const maxFor = (product: CrackerProduct) => (product.inStock ? Math.max(0, product.availableQuantity) : 0);

  const handleQtyChange = (product: CrackerProduct, delta: number) => {
    setQuantities((prev) => {
      const current = Math.min(prev[product.id] || 0, maxFor(product));
      const next = Math.max(0, Math.min(maxFor(product), current + delta));
      return { ...prev, [product.id]: next };
    });
  };

  const selectedItems = allProducts
    .map((product) => ({ product, quantity: Math.min(quantities[product.id] || 0, maxFor(product)) }))
    .filter((item) => item.quantity > 0);

  const totals = computeTotals(selectedItems, rules);
  const {
    // Goods value only; delivery is added at checkout (see the delivery note below).
    subtotal: totalEstimate,
    mrpTotal: totalOriginal,
    savings: totalSavings,
    discountPercent,
    bulkUnlocked,
  } = totals;

  const handleApplyToCart = () => {
    if (selectedItems.length > 0) {
      onAddEstimateToCart(selectedItems);
      setAddedSuccess(true);
      setTimeout(() => {
        setAddedSuccess(false);
        onClose();
      }, 1500);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-4xl bg-white rounded-3xl shadow-2xl border border-slate-200 flex flex-col max-h-[90vh] overflow-hidden text-left"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="p-6 bg-[#0F172A] text-white flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-amber-500/20 text-amber-400 flex items-center justify-center">
              <Calculator className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-white leading-tight">
                Zhi-mart Quick Estimate Sheet
              </h2>
              <p className="text-xs text-slate-400">
                Direct Sivakasi wholesale price calculator{testMode ? '' : ` • ${tierSubtitle(rules)}`}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-2 rounded-full hover:bg-slate-800 text-slate-400 hover:text-white transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Estimation Table */}
        <div className="flex-1 overflow-y-auto p-6">
          {productsQuery.isPending ? (
            <ListSkeleton rows={6} />
          ) : productsQuery.isError && allProducts.length === 0 ? (
            <CatalogErrorCard
              message={errorMessage(productsQuery.error)}
              onRetry={() => void productsQuery.refetch()}
              isRetrying={productsQuery.isFetching}
            />
          ) : allProducts.length === 0 ? (
            <EmptyCatalogState message="No products are available right now." />
          ) : (
          <table className="w-full text-left text-xs sm:text-sm">
            <thead>
              <tr className="border-b border-neutral-200 text-slate-500 font-bold uppercase text-[11px]">
                <th className="pb-3 pl-2">Product Name</th>
                <th className="pb-3">Pack Details</th>
                <th className="pb-3 text-right whitespace-nowrap">
                  Rate
                  {!testMode && hasBulkThreshold(rules) && (
                    <span className="block text-[9px] font-semibold normal-case text-slate-400">
                      ₹{formatRupees(rules.bulkMinOrder)}+ / below
                    </span>
                  )}
                </th>
                <th className="pb-3 text-center">Quantity</th>
                <th className="pb-3 text-right pr-2">Total (₹)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-neutral-100">
              {allProducts.map((prod) => {
                const max = maxFor(prod);
                const qty = Math.min(quantities[prod.id] || 0, max);
                const rowTotal = lineTotal(unitPriceFor(prod, bulkUnlocked), qty);

                return (
                  <tr key={prod.id} className="hover:bg-slate-50/70 transition-colors">
                    <td className="py-3 pl-2 font-semibold text-slate-900">
                      <div>{prod.name}</div>
                      <div className="text-[10px] text-slate-400 uppercase">
                        {prod.category}
                      </div>
                      <StockBadge product={prod} className="mt-1 text-[10px] px-2 py-0.5" />
                    </td>
                    <td className="py-3 text-slate-600 font-medium">
                      {prod.packInfo}
                    </td>
                    <td className="py-3 text-right">
                      <TierRate product={prod} bulkUnlocked={bulkUnlocked} testMode={testMode} />
                      {!testMode && prod.originalPrice > prod.price && (
                        <span className="text-[11px] text-slate-400 line-through block">
                          ₹{formatRupees(prod.originalPrice)}
                        </span>
                      )}
                    </td>
                    <td className="py-3">
                      {max <= 0 ? (
                        <div className="text-center text-[11px] font-bold text-slate-400">Unavailable</div>
                      ) : (
                        <div className="flex items-center justify-center space-x-2 bg-neutral-100 rounded-lg p-1 w-24 mx-auto">
                          <button
                            type="button"
                            onClick={() => handleQtyChange(prod, -1)}
                            disabled={qty <= 0}
                            className="p-1 hover:bg-white rounded text-slate-700 disabled:opacity-30"
                            aria-label="Decrease"
                          >
                            <Minus className="w-3 h-3" />
                          </button>
                          <span className="font-bold text-xs w-6 text-center">
                            {qty}
                          </span>
                          <button
                            type="button"
                            onClick={() => handleQtyChange(prod, 1)}
                            disabled={qty >= max}
                            className="p-1 hover:bg-white rounded text-slate-700 disabled:opacity-30 disabled:cursor-not-allowed"
                            aria-label="Increase"
                          >
                            <Plus className="w-3 h-3" />
                          </button>
                        </div>
                      )}
                    </td>
                    <td className="py-3 text-right pr-2 font-bold text-slate-900">
                      {rowTotal > 0 ? `₹${formatRupees(rowTotal)}` : '—'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          )}
        </div>

        {/* Footer with Subtotals and 1-Click Action */}
        <div className="p-6 bg-slate-50 border-t border-slate-200 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="text-left space-y-1 w-full sm:w-auto sm:max-w-md">
            <p className="text-xs text-slate-500">
              Total Estimate Value:{' '}
              <span className="font-bold text-slate-900">₹{formatRupees(totalEstimate)}</span>{' '}
              <span className="text-slate-400 line-through text-xs">
                (₹{formatRupees(totalOriginal)})
              </span>
            </p>
            <p className="text-xs font-bold text-emerald-600">
              {testMode
                ? 'Test mode — every unit is charged ₹1'
                : `You Save: ₹${formatRupees(totalSavings)} (${discountPercent}% Festival Savings)`}
            </p>
            {deliveryNote(totals) && <p className="text-[11px] font-semibold text-slate-500">{deliveryNote(totals)}</p>}
            {!testMode && <BulkTierSummary totals={totals} />}
          </div>

          <button
            onClick={handleApplyToCart}
            disabled={selectedItems.length === 0}
            className="w-full sm:w-auto bg-[#0F172A] hover:bg-slate-800 disabled:opacity-50 text-white font-bold px-8 py-3 rounded-xl transition-all shadow-md flex items-center justify-center gap-2"
          >
            {addedSuccess ? (
              <>
                <Check className="w-4 h-4 text-emerald-400" />
                <span>Estimate Added to Cart!</span>
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
  );
};
