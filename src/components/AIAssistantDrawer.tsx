import React, { useState } from 'react';
import { X, Bot, Send, Check } from 'lucide-react';
import { ZHI_MART_PRODUCTS } from '../data/crackers';
import { CrackerProduct } from '../types';

interface AIAssistantDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  onAddRecommendedToCart: (products: CrackerProduct[]) => void;
}

export const AIAssistantDrawer: React.FC<AIAssistantDrawerProps> = ({
  isOpen,
  onClose,
  onAddRecommendedToCart,
}) => {
  const [budget, setBudget] = useState('2000');
  const [occasion, setOccasion] = useState('family');
  const [soundPref, setSoundPref] = useState('balanced');
  const [isCalculated, setIsCalculated] = useState(false);
  const [added, setAdded] = useState(false);

  if (!isOpen) return null;

  const handleRecommend = (e: React.FormEvent) => {
    e.preventDefault();
    setIsCalculated(true);
  };

  const recommendedProducts = ZHI_MART_PRODUCTS.slice(0, 4);
  const totalRecommendedPrice = recommendedProducts.reduce((sum, p) => sum + p.price, 0);

  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-black/50 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-md bg-white h-full shadow-2xl flex flex-col justify-between animate-in slide-in-from-right duration-200 border-l border-slate-200 text-left"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="p-6 bg-[#0F172A] text-white flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-amber-500/20 text-amber-400 flex items-center justify-center">
              <Bot className="w-5 h-5" />
            </div>
            <div>
              <h3 className="font-bold text-base text-white">
                Zhi-mart AI Cracker Assistant
              </h3>
              <p className="text-[11px] text-slate-400">
                Custom budget curation for your celebration
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-full hover:bg-slate-800 text-slate-400"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          <form onSubmit={handleRecommend} className="space-y-4">
            <div>
              <label className="text-xs font-bold text-slate-800 block mb-1.5">
                Target Budget (₹)
              </label>
              <div className="grid grid-cols-3 gap-2">
                {['1000', '2500', '5000'].map((b) => (
                  <button
                    key={b}
                    type="button"
                    onClick={() => setBudget(b)}
                    className={`py-2 text-xs font-bold rounded-xl border transition-all ${
                      budget === b
                        ? 'bg-[#0F172A] text-white border-[#0F172A]'
                        : 'border-slate-200 text-slate-700 hover:bg-slate-50'
                    }`}
                  >
                    ₹{b}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <label className="text-xs font-bold text-slate-800 block mb-1.5">
                Celebration Type
              </label>
              <div className="grid grid-cols-3 gap-2">
                {[
                  { id: 'kids', label: 'Kids Friendly' },
                  { id: 'family', label: 'Family Diwali' },
                  { id: 'party', label: 'Loud Party' },
                ].map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setOccasion(item.id)}
                    className={`py-2 text-xs font-bold rounded-xl border transition-all ${
                      occasion === item.id
                        ? 'bg-[#0F172A] text-white border-[#0F172A]'
                        : 'border-slate-200 text-slate-700 hover:bg-slate-50'
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <label className="text-xs font-bold text-slate-800 block mb-1.5">
                Acoustic & Smoke Ratio
              </label>
              <select
                value={soundPref}
                onChange={(e) => setSoundPref(e.target.value)}
                className="w-full text-xs font-semibold px-4 py-2.5 rounded-xl border border-slate-300 outline-none"
              >
                <option value="balanced">Balanced (Fountains + Chakkar + Sound)</option>
                <option value="silent">Eco-Friendly Low Smoke (Silent Fountains)</option>
                <option value="loud">High Thunder (Single Sound & Garlands)</option>
              </select>
            </div>

            <button
              type="submit"
              className="w-full bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] font-bold py-3 rounded-xl transition-all shadow-xs text-xs"
            >
              Generate Best Cracker Combo
            </button>
          </form>

          {isCalculated && (
            <div className="pt-4 border-t border-slate-200 space-y-4 animate-in fade-in">
              <div className="flex items-center justify-between">
                <h4 className="text-xs font-bold text-slate-800 uppercase tracking-wide">
                  AI Recommended Curation
                </h4>
                <span className="text-xs font-extrabold text-[#0F172A]">
                  Total: ₹{totalRecommendedPrice}
                </span>
              </div>

              <div className="space-y-2.5">
                {recommendedProducts.map((p) => (
                  <div
                    key={p.id}
                    className="p-3 bg-slate-50 rounded-xl border border-slate-200 flex items-center justify-between"
                  >
                    <div>
                      <p className="text-xs font-bold text-slate-900">{p.name}</p>
                      <p className="text-[10px] text-slate-500">{p.category}</p>
                    </div>
                    <span className="text-xs font-extrabold text-slate-900">
                      ₹{p.price}
                    </span>
                  </div>
                ))}
              </div>

              <button
                type="button"
                onClick={() => {
                  onAddRecommendedToCart(recommendedProducts);
                  setAdded(true);
                  setTimeout(() => {
                    setAdded(false);
                    onClose();
                  }, 1500);
                }}
                className="w-full bg-[#0F172A] hover:bg-slate-800 text-white font-bold py-3 rounded-xl transition-all shadow-md text-xs flex items-center justify-center gap-2"
              >
                {added ? (
                  <>
                    <Check className="w-4 h-4 text-emerald-400" />
                    <span>Added to Cart!</span>
                  </>
                ) : (
                  <span>Add Entire Combo to Cart</span>
                )}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
