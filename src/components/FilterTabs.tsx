import React from 'react';
import { Flame, Star, Package, Folder, Tag, PartyPopper } from 'lucide-react';
import { FilterTab } from '../types';

interface FilterTabsProps {
  activeTab: FilterTab;
  onSelectTab: (tab: FilterTab) => void;
  onOpenCategoriesModal: () => void;
  onOpenBrandsModal: () => void;
}

export const FilterTabs: React.FC<FilterTabsProps> = ({
  activeTab,
  onSelectTab,
  onOpenCategoriesModal,
  onOpenBrandsModal,
}) => {
  return (
    <div className="w-full bg-[#F8F9FA] pt-6 pb-2 overflow-x-auto no-scrollbar">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 flex items-center space-x-3 text-xs sm:text-sm font-semibold whitespace-nowrap">
        {/* All Products */}
        <button
          onClick={() => onSelectTab('all')}
          className={`px-5 py-2 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            activeTab === 'all'
              ? 'bg-[#0F172A] text-white font-bold'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50'
          }`}
        >
          <span>All Products</span>
        </button>

        {/* Popular */}
        <button
          onClick={() => onSelectTab('popular')}
          className={`px-4 py-2 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            activeTab === 'popular'
              ? 'bg-[#0F172A] text-white font-bold'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50'
          }`}
        >
          <Flame className="w-4 h-4 text-orange-500 fill-orange-500" />
          <span>Popular</span>
        </button>

        {/* Best Selling */}
        <button
          onClick={() => onSelectTab('bestselling')}
          className={`px-4 py-2 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            activeTab === 'bestselling'
              ? 'bg-[#0F172A] text-white font-bold'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50'
          }`}
        >
          <Star className="w-4 h-4 text-amber-500 fill-amber-500" />
          <span>Best Selling</span>
        </button>

        {/* New Arrivals */}
        <button
          onClick={() => onSelectTab('newarrivals')}
          className={`px-4 py-2 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            activeTab === 'newarrivals'
              ? 'bg-[#0F172A] text-white font-bold'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50'
          }`}
        >
          <Package className="w-4 h-4 text-blue-500" />
          <span>New Arrivals</span>
        </button>

        {/* Party / Functions */}
        <button
          onClick={() => onSelectTab('party')}
          className={`px-4 py-2 rounded-full transition-all flex items-center gap-1.5 shadow-2xs ${
            activeTab === 'party'
              ? 'bg-[#0F172A] text-white font-bold'
              : 'bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50'
          }`}
        >
          <PartyPopper className="w-4 h-4 text-purple-500" />
          <span>Party / Functions</span>
        </button>

        {/* Categories By Product */}
        <button
          onClick={onOpenCategoriesModal}
          className="px-4 py-2 rounded-full bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50 transition-all flex items-center gap-1.5 shadow-2xs"
        >
          <Folder className="w-4 h-4 text-slate-500" />
          <span>Categories By Product</span>
        </button>

        {/* Brands By Product */}
        <button
          onClick={onOpenBrandsModal}
          className="px-4 py-2 rounded-full bg-white border border-neutral-200 text-neutral-700 hover:bg-neutral-50 transition-all flex items-center gap-1.5 shadow-2xs"
        >
          <Tag className="w-4 h-4 text-amber-600" />
          <span>Brands By Product</span>
        </button>
      </div>
    </div>
  );
};
