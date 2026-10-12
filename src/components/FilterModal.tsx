import React, { useState } from 'react';
import { X, SlidersHorizontal, Check } from 'lucide-react';
import { CrackerCategory } from '../types';

interface FilterModalProps {
  isOpen: boolean;
  onClose: () => void;
  selectedCategory: CrackerCategory;
  onApplyFilters: (filters: {
    category: CrackerCategory;
    minPrice: number;
    maxPrice: number;
    ecoOnly: boolean;
  }) => void;
}

export const FilterModal: React.FC<FilterModalProps> = ({
  isOpen,
  onClose,
  selectedCategory,
  onApplyFilters,
}) => {
  const [category, setCategory] = useState<CrackerCategory>(selectedCategory);
  const [minPrice, setMinPrice] = useState(1000);
  const [maxPrice, setMaxPrice] = useState(5000);
  const [ecoOnly, setEcoOnly] = useState(false);

  if (!isOpen) return null;

  const handleApply = () => {
    onApplyFilters({
      category,
      minPrice,
      maxPrice,
      ecoOnly,
    });
    onClose();
  };

  const categories: { label: string; value: CrackerCategory }[] = [
    { label: 'All Varieties', value: 'all' },
    { label: 'Aerial Shots', value: 'aerial' },
    { label: 'Ground Chakras', value: 'spinners' },
    { label: 'Flower Pots', value: 'fountains' },
    { label: 'Gift Hampers', value: 'hampers' },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-lg bg-white rounded-3xl shadow-2xl border border-neutral-200 p-8 text-left animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between pb-5 border-b border-neutral-100">
          <div className="flex items-center gap-2.5">
            <SlidersHorizontal className="w-4 h-4 text-[#35383F]" />
            <h2 className="text-lg font-bold text-[#23262C]">Filters</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-full hover:bg-neutral-100 text-[#4E525C] transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Filters Content */}
        <div className="py-6 space-y-6">
          {/* Price Range */}
          <div>
            <div className="flex justify-between items-center mb-2">
              <span className="text-xs font-bold text-[#292C32]">Price Range</span>
              <span className="text-xs font-semibold text-[#6E737F]">
                ${minPrice} - ${maxPrice} USD
              </span>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="text-[10px] font-semibold text-neutral-400">Min Price ($)</label>
                <input
                  type="number"
                  value={minPrice}
                  onChange={(e) => setMinPrice(Number(e.target.value))}
                  className="w-full text-xs font-semibold px-3 py-2 border rounded-xl"
                />
              </div>
              <div>
                <label className="text-[10px] font-semibold text-neutral-400">Max Price ($)</label>
                <input
                  type="number"
                  value={maxPrice}
                  onChange={(e) => setMaxPrice(Number(e.target.value))}
                  className="w-full text-xs font-semibold px-3 py-2 border rounded-xl"
                />
              </div>
            </div>
          </div>

          {/* Categories */}
          <div>
            <label className="block text-xs font-bold text-[#292C32] mb-3">
              Pyrotechnic Variety
            </label>
            <div className="flex flex-wrap gap-2">
              {categories.map((c) => (
                <button
                  key={c.value}
                  type="button"
                  onClick={() => setCategory(c.value)}
                  className={`text-xs font-medium px-4 py-2 rounded-full border transition-all ${
                    category === c.value
                      ? 'bg-[#373A40] text-white border-[#373A40]'
                      : 'border-neutral-200 text-[#4C505A] hover:bg-neutral-50'
                  }`}
                >
                  {c.label}
                </button>
              ))}
            </div>
          </div>

          {/* Eco Only Checkbox */}
          <div className="pt-2">
            <label
              onClick={() => setEcoOnly(!ecoOnly)}
              className="flex items-center gap-3 cursor-pointer select-none"
            >
              <div
                className={`w-5 h-5 rounded-md border flex items-center justify-center transition-colors ${
                  ecoOnly
                    ? 'bg-[#373A40] border-[#373A40] text-white'
                    : 'border-neutral-300 bg-white'
                }`}
              >
                {ecoOnly && <Check className="w-3.5 h-3.5" />}
              </div>
              <div>
                <p className="text-xs font-bold text-[#27292F]">
                  CSIR-NEERI Green Certified Only
                </p>
                <p className="text-[11px] text-[#787D8A]">
                  Zero barium, strictly verified low-emission formulations
                </p>
              </div>
            </label>
          </div>
        </div>

        {/* Actions */}
        <div className="flex items-center justify-end gap-3 pt-5 border-t border-neutral-100">
          <button
            type="button"
            onClick={() => {
              setCategory('all');
              setMinPrice(1000);
              setMaxPrice(5000);
              setEcoOnly(false);
            }}
            className="text-xs font-semibold text-[#666B76] hover:text-black px-4 py-2.5"
          >
            Reset All
          </button>
          <button
            type="button"
            onClick={handleApply}
            className="bg-[#373A40] hover:bg-[#25272B] text-white text-xs font-bold px-6 py-2.5 rounded-full shadow-sm"
          >
            Apply Filters
          </button>
        </div>
      </div>
    </div>
  );
};
