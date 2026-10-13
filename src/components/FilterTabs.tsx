import React from 'react';

interface FilterTabsProps {
  selectedCategory: string;
  onSelectCategory: (category: string) => void;
  categories: { name: string; count: number }[];
}

export const FilterTabs: React.FC<FilterTabsProps> = ({
  selectedCategory,
  onSelectCategory,
  categories = [],
}) => {
  return (
    <div className="w-full bg-[#F8F9FA] pt-6 pb-2 overflow-x-auto no-scrollbar">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 flex items-center space-x-2.5 text-xs sm:text-sm font-semibold whitespace-nowrap">
        {/* All Products */}
        <button
          type="button"
          onClick={() => onSelectCategory('all')}
          className={`px-5 py-2.5 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            selectedCategory === 'all'
              ? 'bg-[#0F172A] text-white font-bold shadow-xs'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50 hover:border-neutral-300'
          }`}
        >
          <span>All Products</span>
        </button>

        {/* Categories List */}
        {categories.map((cat) => {
          const isSelected = selectedCategory.toLowerCase() === cat.name.toLowerCase();
          return (
            <button
              key={cat.name}
              type="button"
              onClick={() => onSelectCategory(cat.name)}
              className={`px-4 py-2.5 rounded-full transition-all flex items-center gap-2 shadow-2xs ${
                isSelected
                  ? 'bg-[#0F172A] text-white font-bold shadow-xs'
                  : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50 hover:border-neutral-300'
              }`}
            >
              <span>{cat.name}</span>
              <span
                className={`text-[10px] px-1.5 py-0.5 rounded-full font-bold ${
                  isSelected
                    ? 'bg-white/20 text-white'
                    : 'bg-slate-100 text-slate-500'
                }`}
              >
                {cat.count}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
};
