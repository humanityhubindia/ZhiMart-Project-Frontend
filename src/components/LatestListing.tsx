import React, { useState } from 'react';
import { Heart } from 'lucide-react';
import { CrackerItem } from '../types';

interface LatestListingProps {
  items: CrackerItem[];
  onSelectItem: (item: CrackerItem) => void;
}

export const LatestListing: React.FC<LatestListingProps> = ({
  items,
  onSelectItem,
}) => {
  const [wishlist, setWishlist] = useState<Record<string, boolean>>({});

  const toggleWishlist = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setWishlist((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  // 4 items matching Screenshot 1 titles
  const displayItems = [
    {
      item: items[0] || items[0],
      displayTitle: 'Well Furnished Apartment',
      displayLocation: '100 Smart Street, LA, USA',
      crackerName: 'Artisanal Celestial Aerial Fireworks',
    },
    {
      item: items[1] || items[0],
      displayTitle: 'Comfortable Family Flat',
      displayLocation: '100 Smart Street, LA, USA',
      crackerName: 'Aurora Artisanal Ground Spinner',
    },
    {
      item: items[2] || items[0],
      displayTitle: 'Beach House Summer',
      displayLocation: '100 Smart Street, LA, USA',
      crackerName: 'Fiore Fountain Flower Pot Suite',
    },
    {
      item: items[3] || items[0],
      displayTitle: 'Dobule Size Room',
      displayLocation: '100 Smart Street, LA, USA',
      crackerName: 'The Celebration Keepsake Hamper',
    },
  ];

  return (
    <section className="max-w-7xl mx-auto px-6 py-16">
      {/* Heading with accent underline bar matching Screenshot 1 */}
      <div className="mb-10 text-left">
        <h2 className="text-3xl font-extrabold text-[#282B30] tracking-tight leading-tight">
          Latest on the
          <br />
          Property Listing
        </h2>
        <div className="w-24 h-1.5 bg-[#3B3E44] rounded-full mt-3" />
      </div>

      {/* 4 Cards Grid */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6">
        {displayItems.map(({ item, displayTitle, displayLocation }, index) => {
          if (!item) return null;
          const isLiked = wishlist[item.id] || false;

          return (
            <div
              key={`${item.id}-${index}`}
              onClick={() => onSelectItem(item)}
              className="group cursor-pointer flex flex-col"
            >
              {/* Card Container with subtle grey background matching Screenshot 1 */}
              <div className="relative w-full aspect-[4/5] bg-[#DEE2E6] rounded-2xl p-5 flex flex-col justify-between overflow-hidden transition-all duration-200 group-hover:shadow-md group-hover:-translate-y-1">
                {/* Product image subtle preview background */}
                {item.image && (
                  <img
                    src={item.image}
                    alt={displayTitle}
                    className="absolute inset-0 w-full h-full object-cover opacity-85 transition-transform duration-300 group-hover:scale-105"
                  />
                )}

                {/* Subtle gradient overlay to match clean neutral aesthetic */}
                <div className="absolute inset-0 bg-gradient-to-b from-black/10 via-transparent to-black/30 pointer-events-none" />

                {/* Top Right Heart Wishlist Icon */}
                <div className="relative z-10 self-end">
                  <button
                    type="button"
                    onClick={(e) => toggleWishlist(item.id, e)}
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

                {/* Bottom Left Circular Avatar / Badge matching Screenshot 1 */}
                <div className="relative z-10 self-start">
                  <div className="w-12 h-12 rounded-full bg-[#B2BAC4] border-2 border-white/80 shadow-xs flex items-center justify-center overflow-hidden">
                    <span className="text-xs font-bold text-white">PYRO</span>
                  </div>
                </div>
              </div>

              {/* Text metadata below card matching Screenshot 1 */}
              <div className="pt-3 text-left">
                <h3 className="font-bold text-[15px] text-[#292C32] leading-snug group-hover:text-black transition-colors">
                  {displayTitle}
                </h3>
                <p className="text-xs text-[#8E939E] font-medium mt-1">
                  {displayLocation}
                </p>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
};
