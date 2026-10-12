import React, { useState } from 'react';
import { Search } from 'lucide-react';
import { CrackerCategory } from '../types';

interface HeroSearchProps {
  onSearch: (params: {
    category: CrackerCategory;
    searchQuery: string;
    occasion: string;
    quantity: string;
  }) => void;
  selectedCategory: CrackerCategory;
  onSelectCategory: (category: CrackerCategory) => void;
}

export const HeroSearch: React.FC<HeroSearchProps> = ({
  onSearch,
  selectedCategory,
  onSelectCategory,
}) => {
  const [activeTab, setActiveTab] = useState<'Rooms' | 'Flats' | 'Hostels' | 'Villas'>('Rooms');
  const [locationInput, setLocationInput] = useState('');
  const [checkInInput, setCheckInInput] = useState('');
  const [checkOutInput, setCheckOutInput] = useState('');
  const [guestsInput, setGuestsInput] = useState('');

  const handleTabClick = (tab: 'Rooms' | 'Flats' | 'Hostels' | 'Villas') => {
    setActiveTab(tab);
    if (tab === 'Rooms') onSelectCategory('all');
    if (tab === 'Flats') onSelectCategory('aerial');
    if (tab === 'Hostels') onSelectCategory('spinners');
    if (tab === 'Villas') onSelectCategory('hampers');
  };

  const handleExecuteSearch = (e: React.FormEvent) => {
    e.preventDefault();
    onSearch({
      category: selectedCategory,
      searchQuery: locationInput,
      occasion: checkInInput,
      quantity: guestsInput,
    });
  };

  return (
    <section className="relative w-full bg-[#ECEEF1] pt-20 pb-28 px-4 overflow-hidden">
      {/* Background BANNER typography as in Screenshot 1 */}
      <div className="absolute inset-0 flex items-center justify-center pointer-events-none select-none opacity-40">
        <span className="text-[120px] md:text-[180px] font-black text-[#D8DCE2] tracking-[0.2em] uppercase leading-none">
          BANNER
        </span>
      </div>

      <div className="relative max-w-5xl mx-auto flex flex-col items-center">
        {/* FIND Header & Tabs row matching Screenshot 1 */}
        <div className="w-full flex flex-col sm:flex-row items-center justify-start gap-8 mb-6 px-4">
          <h2 className="text-3xl font-black tracking-tight text-[#2D3036] uppercase">
            FIND
          </h2>

          <div className="flex items-center space-x-6 text-sm font-semibold text-[#5A5E66]">
            {(['Rooms', 'Flats', 'Hostels', 'Villas'] as const).map((tab) => {
              const isActive = activeTab === tab;
              return (
                <button
                  key={tab}
                  type="button"
                  onClick={() => handleTabClick(tab)}
                  className={`relative py-1 transition-colors ${
                    isActive ? 'text-[#24272D]' : 'hover:text-[#24272D]'
                  }`}
                >
                  {tab}
                  {isActive && (
                    <span className="absolute bottom-0 left-0 w-full h-[2px] bg-[#24272D]" />
                  )}
                </button>
              );
            })}
          </div>
        </div>

        {/* Floating Pill Search Bar matching Screenshot 1 */}
        <form
          onSubmit={handleExecuteSearch}
          className="w-full bg-white rounded-full shadow-[0_4px_25px_rgba(0,0,0,0.06)] border border-neutral-100 px-6 py-3 flex flex-col md:flex-row items-center justify-between gap-2"
        >
          {/* Section 1: Location */}
          <div className="flex-1 w-full px-4 py-1 text-left border-b md:border-b-0 md:border-r border-neutral-200">
            <label className="block text-[11px] font-bold text-[#2B2E35] tracking-tight">
              Location
            </label>
            <input
              type="text"
              value={locationInput}
              onChange={(e) => setLocationInput(e.target.value)}
              placeholder="Which city do you prefer?"
              className="w-full text-xs text-[#525660] placeholder:text-[#A0A4AE] bg-transparent outline-none pt-0.5"
            />
          </div>

          {/* Section 2: Check In */}
          <div className="flex-1 w-full px-4 py-1 text-left border-b md:border-b-0 md:border-r border-neutral-200">
            <label className="block text-[11px] font-bold text-[#2B2E35] tracking-tight">
              Check In
            </label>
            <input
              type="text"
              value={checkInInput}
              onChange={(e) => setCheckInInput(e.target.value)}
              placeholder="Add Dates"
              className="w-full text-xs text-[#525660] placeholder:text-[#A0A4AE] bg-transparent outline-none pt-0.5"
            />
          </div>

          {/* Section 3: Check Out */}
          <div className="flex-1 w-full px-4 py-1 text-left border-b md:border-b-0 md:border-r border-neutral-200">
            <label className="block text-[11px] font-bold text-[#2B2E35] tracking-tight">
              Check Out
            </label>
            <input
              type="text"
              value={checkOutInput}
              onChange={(e) => setCheckOutInput(e.target.value)}
              placeholder="Add Dates"
              className="w-full text-xs text-[#525660] placeholder:text-[#A0A4AE] bg-transparent outline-none pt-0.5"
            />
          </div>

          {/* Section 4: Guests */}
          <div className="flex-1 w-full px-4 py-1 text-left">
            <label className="block text-[11px] font-bold text-[#2B2E35] tracking-tight">
              Guests
            </label>
            <input
              type="text"
              value={guestsInput}
              onChange={(e) => setGuestsInput(e.target.value)}
              placeholder="Add Guests"
              className="w-full text-xs text-[#525660] placeholder:text-[#A0A4AE] bg-transparent outline-none pt-0.5"
            />
          </div>

          {/* Circular Search Button */}
          <button
            type="submit"
            className="w-12 h-12 rounded-full bg-[#3D4046] hover:bg-[#2A2C31] text-white flex items-center justify-center shrink-0 transition-transform active:scale-95 shadow-sm ml-2"
            aria-label="Search"
          >
            <Search className="w-5 h-5" />
          </button>
        </form>
      </div>
    </section>
  );
};
