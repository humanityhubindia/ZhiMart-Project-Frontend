import React, { useState } from 'react';
import { Heart, SlidersHorizontal } from 'lucide-react';
import { CrackerItem } from '../types';

interface ListingCatalogProps {
  items: CrackerItem[];
  onSelectItem: (item: CrackerItem) => void;
  onOpenFilterModal: () => void;
}

export const ListingCatalog: React.FC<ListingCatalogProps> = ({
  items,
  onSelectItem,
  onOpenFilterModal,
}) => {
  const [activeCategory, setActiveCategory] = useState<string>('Apartments');
  const [wishlist, setWishlist] = useState<Record<string, boolean>>({});

  const categories = [
    'Apartments',
    'Houses',
    'Villas',
    'Homestays',
    'Combos',
    'More',
  ];

  const toggleWishlist = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setWishlist((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  // Replicate items in grid to match screenshot 2
  const catalogGrid = [
    {
      item: items[0] || items[0],
      title: 'Well Furnished Apartment',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
    {
      item: items[1] || items[0],
      title: 'Amazing Dream Building',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
    {
      item: items[2] || items[0],
      title: 'Boys Hostel Apartment',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
    {
      item: items[3] || items[0],
      title: 'Well Furnished Apartment',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
    {
      item: items[4] || items[1] || items[0],
      title: 'Amazing Dream Building',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
    {
      item: items[5] || items[2] || items[0],
      title: 'Boys Hostel Apartment',
      location: '100 Smart Street, LA, USA',
      price: '$ 1000 - 5000 USD',
    },
  ];

  return (
    <section id="catalog" className="max-w-7xl mx-auto px-6 py-12">
      {/* Category Pills & Filter Button Row matching Screenshot 2 */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-8">
        {/* Categories Bar */}
        <div className="flex flex-wrap items-center gap-3 text-sm font-semibold text-[#575B63]">
          {categories.map((cat, idx) => {
            const isActive = activeCategory === cat;
            return (
              <React.Fragment key={cat}>
                <button
                  type="button"
                  onClick={() => setActiveCategory(cat)}
                  className={`relative py-1 transition-colors ${
                    isActive ? 'text-[#23262C]' : 'hover:text-[#23262C]'
                  }`}
                >
                  {cat}
                  {isActive && (
                    <span className="absolute bottom-0 left-0 w-full h-[2px] bg-[#23262C]" />
                  )}
                </button>
                {idx < categories.length - 1 && (
                  <span className="text-[#A2A7B0] select-none">•</span>
                )}
              </React.Fragment>
            );
          })}
        </div>

        {/* Filters Button with Sliders Icon matching Screenshot 2 */}
        <button
          type="button"
          onClick={onOpenFilterModal}
          className="flex items-center gap-2 border border-[#C5CAD2] hover:border-neutral-500 bg-white text-[#2B2E35] text-xs font-semibold px-4 py-2 rounded-full transition-all shadow-xs active:scale-95"
        >
          <SlidersHorizontal className="w-3.5 h-3.5" />
          <span>Filters</span>
        </button>
      </div>

      {/* 3-Column Cards Grid matching Screenshot 2 */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-8">
        {catalogGrid.map(({ item, title, location, price }, index) => {
          if (!item) return null;
          const isLiked = wishlist[`${item.id}-${index}`] || false;

          return (
            <div
              key={`${item.id}-${index}`}
              onClick={() => onSelectItem(item)}
              className="group cursor-pointer flex flex-col text-left"
            >
              {/* Card Container with subtle neutral background & overlay pills */}
              <div className="relative w-full aspect-[4/3.5] bg-[#DEE2E6] rounded-2xl overflow-hidden p-5 flex flex-col justify-between transition-all duration-200 group-hover:shadow-lg group-hover:-translate-y-1">
                {/* Product Photo */}
                {item.image && (
                  <img
                    src={item.image}
                    alt={title}
                    className="absolute inset-0 w-full h-full object-cover opacity-90 transition-transform duration-300 group-hover:scale-105"
                  />
                )}

                {/* Subtle dark gradient overlay */}
                <div className="absolute inset-0 bg-gradient-to-b from-black/15 via-transparent to-black/40 pointer-events-none" />

                {/* Top Right Heart Wishlist Icon matching Screenshot 2 */}
                <div className="relative z-10 self-end">
                  <button
                    type="button"
                    onClick={(e) => toggleWishlist(`${item.id}-${index}`, e)}
                    className="p-2 rounded-full hover:bg-black/10 transition-colors"
                    aria-label="Save to Wishlist"
                  >
                    <Heart
                      className={`w-5 h-5 transition-colors ${
                        isLiked
                          ? 'fill-red-500 text-red-500'
                          : 'text-[#4A4E55] hover:text-[#1E1E1E]'
                      }`}
                    />
                  </button>
                </div>

                {/* Bottom Overlay Row: Price Pill & Carousel Dots matching Screenshot 2 */}
                <div className="relative z-10 flex items-center justify-between w-full pt-4">
                  <span className="text-xs font-semibold text-[#32363D] bg-white/80 backdrop-blur-sm px-3 py-1 rounded-full shadow-xs">
                    {price}
                  </span>

                  {/* 4 dots matching Screenshot 2 */}
                  <div className="flex items-center gap-1.5 px-2 py-1 rounded-full bg-black/20 backdrop-blur-xs">
                    <span className="w-1.5 h-1.5 rounded-full bg-white" />
                    <span className="w-1.5 h-1.5 rounded-full bg-white/60" />
                    <span className="w-1.5 h-1.5 rounded-full bg-white/60" />
                    <span className="w-1.5 h-1.5 rounded-full bg-white/60" />
                  </div>
                </div>
              </div>

              {/* Text metadata below card matching Screenshot 2 */}
              <div className="pt-3">
                <h3 className="font-bold text-base text-[#292C32] leading-snug group-hover:text-black transition-colors">
                  {title}
                </h3>
                <p className="text-xs text-[#8E939E] font-medium mt-1">
                  {location}
                </p>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
};
