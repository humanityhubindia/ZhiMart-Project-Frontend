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
} from 'lucide-react';
import { ZHI_MART_PRODUCTS } from '../data/crackers';
import { CrackerProduct } from '../types';

interface QuickEstimatePageProps {
  onBack: () => void;
  onAddEstimateToCart: (items: { product: CrackerProduct; quantity: number }[]) => void;
}

export const QuickEstimatePage: React.FC<QuickEstimatePageProps> = ({
  onBack,
  onAddEstimateToCart,
}) => {
  // Preset default quantities matching the screenshot
  const [quantities, setQuantities] = useState<Record<string, number>>({
    'zm-01': 2,
    'zm-02': 1,
    'zm-05': 1,
  });
  const [selectedCategory, setSelectedCategory] = useState<string>('all');
  const [addedSuccess, setAddedSuccess] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);

  const categories = [
    { label: 'All Crackers', value: 'all' },
    { label: 'Single Sound', value: 'SINGLE SOUND CRACKERS' },
    { label: 'Electric Tape', value: 'ELECTRIC TAPE FUSE CRACKERS' },
    { label: 'Deluxe Wire Fuse', value: 'DELUXE WIRE FUSE CRACKERS' },
    { label: 'Flower Pots', value: 'FLOWER POTS' },
    { label: 'Ground Chakkars', value: 'GROUND CHAKKARS' },
    { label: 'Aerial Repeaters', value: 'AERIAL REPEATERS' },
  ];

  const filteredProducts = selectedCategory === 'all'
    ? ZHI_MART_PRODUCTS
    : ZHI_MART_PRODUCTS.filter((p) => p.category === selectedCategory);

  const handleQtyChange = (id: string, delta: number) => {
    setQuantities((prev) => {
      const current = prev[id] || 0;
      const next = Math.max(0, current + delta);
      return { ...prev, [id]: next };
    });
  };

  const handleSetQty = (id: string, value: number) => {
    setQuantities((prev) => ({
      ...prev,
      [id]: Math.max(0, value),
    }));
  };

  const handleResetAll = () => {
    setQuantities({});
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
  const totalPacksCount = selectedItems.reduce((acc, item) => acc + item.quantity, 0);

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
      `Zhi-mart Diwali Estimate: ${selectedItems.length} items, Total: ₹${totalEstimate} (Saved ₹${totalSavings})!\n${window.location.href}`
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
              Direct Sivakasi wholesale price calculator • Flat 45% Festival Discount
            </p>
          </div>
        </div>

        <div className="bg-amber-50 border border-amber-200/80 px-4 py-2 rounded-2xl flex items-center gap-3 text-xs">
          <div>
            <span className="text-[10px] text-amber-700 uppercase font-black tracking-wide block">
              Estimated Total
            </span>
            <span className="text-lg font-black text-amber-950">
              ₹{totalEstimate}
            </span>
          </div>
          <div className="h-8 w-px bg-amber-200" />
          <div>
            <span className="text-[10px] text-emerald-700 uppercase font-black tracking-wide block">
              You Save (45%)
            </span>
            <span className="text-sm font-extrabold text-emerald-600">
              ₹{totalSavings}
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
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-slate-200/80 bg-slate-50/70 text-slate-500 font-bold uppercase text-[11px] tracking-wider">
                <th className="py-4 pl-6">PRODUCT NAME</th>
                <th className="py-4 px-4">PACK DETAILS</th>
                <th className="py-4 px-4 text-right">RATE</th>
                <th className="py-4 px-4 text-center min-w-[140px]">QUANTITY</th>
                <th className="py-4 pr-6 text-right">TOTAL (₹)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {filteredProducts.map((prod) => {
                const qty = quantities[prod.id] || 0;
                const rowTotal = prod.price * qty;

                return (
                  <tr
                    key={prod.id}
                    className={`transition-colors hover:bg-slate-50/80 ${
                      qty > 0 ? 'bg-amber-50/20' : ''
                    }`}
                  >
                    {/* Product Name */}
                    <td className="py-4 pl-6">
                      <div className="flex items-center gap-3">
                        <img
                          src={prod.image}
                          alt={prod.name}
                          className="w-10 h-10 object-contain rounded-lg border border-slate-100 p-0.5 bg-white shrink-0"
                        />
                        <div>
                          <div className="font-bold text-slate-900 text-sm leading-snug">
                            {prod.name}
                          </div>
                          <div className="text-[10px] font-bold text-slate-400 uppercase tracking-wider mt-0.5">
                            {prod.category}
                          </div>
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
                      <div className="font-extrabold text-slate-900 text-sm">
                        ₹{prod.price}
                      </div>
                      <div className="text-xs text-slate-400 line-through">
                        ₹{prod.originalPrice}
                      </div>
                    </td>

                    {/* Quantity Stepper matching user's screenshot */}
                    <td className="py-4 px-4">
                      <div className="flex items-center justify-center space-x-2 bg-slate-100 rounded-xl p-1 max-w-[120px] mx-auto">
                        <button
                          type="button"
                          onClick={() => handleQtyChange(prod.id, -1)}
                          className="p-1 hover:bg-white rounded-lg text-slate-700 active:scale-90 transition-all"
                          aria-label="Decrease"
                        >
                          <Minus className="w-3.5 h-3.5" />
                        </button>
                        <input
                          type="number"
                          min="0"
                          value={qty}
                          onChange={(e) => handleSetQty(prod.id, parseInt(e.target.value) || 0)}
                          className="w-10 text-center font-bold text-xs bg-transparent outline-none"
                        />
                        <button
                          type="button"
                          onClick={() => handleQtyChange(prod.id, 1)}
                          className="p-1 hover:bg-white rounded-lg text-slate-700 active:scale-90 transition-all"
                          aria-label="Increase"
                        >
                          <Plus className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </td>

                    {/* Total (₹) */}
                    <td className="py-4 pr-6 text-right font-black text-slate-900 text-sm">
                      {rowTotal > 0 ? (
                        <span className="text-slate-900">₹{rowTotal}</span>
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

        {/* 5. Sticky Bottom Summary Bar matching user's screenshot */}
        <div className="p-6 bg-white border-t border-slate-200/90 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="text-left space-y-1">
            <div className="text-sm font-semibold text-slate-700">
              Total Estimate Value:{' '}
              <span className="font-black text-lg text-slate-900">₹{totalEstimate}</span>{' '}
              <span className="text-xs text-slate-400 line-through font-normal">
                (₹{totalOriginal})
              </span>
            </div>
            <div className="text-xs font-bold text-emerald-600">
              You Save: ₹{totalSavings} (45% Festival Savings)
            </div>
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
