import React, { useState } from 'react';
import { X, Calculator, Plus, Minus, Check, ShoppingBag } from 'lucide-react';
import { ZHI_MART_PRODUCTS } from '../data/crackers';
import { CrackerProduct } from '../types';

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
  const [quantities, setQuantities] = useState<Record<string, number>>({
    'zm-01': 2,
    'zm-02': 1,
    'zm-05': 1,
    'zm-10': 3,
    'zm-11': 2,
  });
  const [addedSuccess, setAddedSuccess] = useState(false);

  if (!isOpen) return null;

  const handleQtyChange = (id: string, delta: number) => {
    setQuantities((prev) => {
      const current = prev[id] || 0;
      const next = Math.max(0, current + delta);
      return { ...prev, [id]: next };
    });
  };

  const selectedItems = ZHI_MART_PRODUCTS.filter((p) => (quantities[p.id] || 0) > 0).map(
    (product) => ({
      product,
      quantity: quantities[product.id] || 0,
    })
  );

  const totalEstimate = selectedItems.reduce(
    (acc, item) => acc + item.product.price * item.quantity,
    0
  );

  const totalOriginal = selectedItems.reduce(
    (acc, item) => acc + item.product.originalPrice * item.quantity,
    0
  );

  const totalSavings = totalOriginal - totalEstimate;

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
                Direct Sivakasi wholesale price calculator • Flat 45% Festival Discount
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
          <table className="w-full text-left text-xs sm:text-sm">
            <thead>
              <tr className="border-b border-neutral-200 text-slate-500 font-bold uppercase text-[11px]">
                <th className="pb-3 pl-2">Product Name</th>
                <th className="pb-3">Pack Details</th>
                <th className="pb-3 text-right">Rate</th>
                <th className="pb-3 text-center">Quantity</th>
                <th className="pb-3 text-right pr-2">Total (₹)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-neutral-100">
              {ZHI_MART_PRODUCTS.map((prod) => {
                const qty = quantities[prod.id] || 0;
                const rowTotal = prod.price * qty;

                return (
                  <tr key={prod.id} className="hover:bg-slate-50/70 transition-colors">
                    <td className="py-3 pl-2 font-semibold text-slate-900">
                      <div>{prod.name}</div>
                      <div className="text-[10px] text-slate-400 uppercase">
                        {prod.category}
                      </div>
                    </td>
                    <td className="py-3 text-slate-600 font-medium">
                      {prod.packInfo}
                    </td>
                    <td className="py-3 text-right">
                      <span className="font-bold text-slate-900">₹{prod.price}</span>
                      <span className="text-[11px] text-slate-400 line-through block">
                        ₹{prod.originalPrice}
                      </span>
                    </td>
                    <td className="py-3">
                      <div className="flex items-center justify-center space-x-2 bg-neutral-100 rounded-lg p-1 w-24 mx-auto">
                        <button
                          type="button"
                          onClick={() => handleQtyChange(prod.id, -1)}
                          className="p-1 hover:bg-white rounded text-slate-700"
                        >
                          <Minus className="w-3 h-3" />
                        </button>
                        <span className="font-bold text-xs w-6 text-center">
                          {qty}
                        </span>
                        <button
                          type="button"
                          onClick={() => handleQtyChange(prod.id, 1)}
                          className="p-1 hover:bg-white rounded text-slate-700"
                        >
                          <Plus className="w-3 h-3" />
                        </button>
                      </div>
                    </td>
                    <td className="py-3 text-right pr-2 font-bold text-slate-900">
                      {rowTotal > 0 ? `₹${rowTotal}` : '—'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>

        {/* Footer with Subtotals and 1-Click Action */}
        <div className="p-6 bg-slate-50 border-t border-slate-200 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="text-left">
            <p className="text-xs text-slate-500">
              Total Estimate Value:{' '}
              <span className="font-bold text-slate-900">₹{totalEstimate}</span>{' '}
              <span className="text-slate-400 line-through text-xs">
                (₹{totalOriginal})
              </span>
            </p>
            <p className="text-xs font-bold text-emerald-600">
              You Save: ₹{totalSavings} (45% Festival Savings)
            </p>
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
