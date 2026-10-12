import React, { useState } from 'react';
import {
  Heart,
  Share2,
  Bed,
  Bath,
  Car,
  Dog,
  Utensils,
  Tv,
  Wind,
  Wifi,
  Disc,
  Home,
  Building,
  Phone,
  ArrowLeft,
  Check,
} from 'lucide-react';
import { CrackerItem } from '../types';

interface ProductDetailViewProps {
  item: CrackerItem;
  onBack: () => void;
  onAddToCart: (item: CrackerItem, tierIndex: number) => void;
  onOpenInquiry: (item: CrackerItem) => void;
}

export const ProductDetailView: React.FC<ProductDetailViewProps> = ({
  item,
  onBack,
  onAddToCart,
  onOpenInquiry,
}) => {
  const [isLiked, setIsLiked] = useState(false);
  const [selectedTierIndex, setSelectedTierIndex] = useState(0);
  const [copiedShare, setCopiedShare] = useState(false);
  const [reservedSuccess, setReservedSuccess] = useState(false);

  const handleShare = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(window.location.href);
      setCopiedShare(true);
      setTimeout(() => setCopiedShare(false), 2000);
    }
  };

  const handleReserve = () => {
    onAddToCart(item, selectedTierIndex);
    setReservedSuccess(true);
    setTimeout(() => setReservedSuccess(false), 2500);
  };

  return (
    <div className="max-w-7xl mx-auto px-6 py-8 animate-in fade-in duration-200">
      {/* Back button */}
      <button
        onClick={onBack}
        className="flex items-center gap-2 text-sm font-semibold text-[#545862] hover:text-[#18181B] mb-6 transition-colors"
      >
        <ArrowLeft className="w-4 h-4" />
        <span>Back to listings</span>
      </button>

      {/* Top 5-Image Gallery matching Screenshot 4 */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 rounded-3xl overflow-hidden mb-12">
        {/* Left Hero Image */}
        <div className="relative aspect-[4/3] md:aspect-auto md:h-full bg-[#DEE2E6] rounded-2xl md:rounded-l-2xl overflow-hidden min-h-[360px] md:min-h-[460px]">
          <img
            src={item.gallery[0] || item.image}
            alt={item.title}
            className="w-full h-full object-cover"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent pointer-events-none" />

          {/* Author Badge Overlay matching Screenshot 4 bottom-left */}
          <div className="absolute bottom-6 left-6 flex items-center space-x-3.5 z-10 text-white">
            <div className="w-12 h-12 rounded-full bg-white shadow-sm flex items-center justify-center overflow-hidden">
              <span className="text-xs font-bold text-[#2A2C31]">JD</span>
            </div>
            <div className="text-left drop-shadow-sm">
              <p className="text-[11px] text-neutral-300 font-medium">Listed By:</p>
              <h4 className="text-sm font-bold text-white leading-tight">
                {item.artisan || 'John Doberman'}
              </h4>
              <p className="text-xs text-neutral-200 font-semibold mt-0.5">
                For: $ 1000 - $ 5000
              </p>
            </div>
          </div>
        </div>

        {/* Right 2x2 Grid of 4 Images matching Screenshot 4 */}
        <div className="grid grid-cols-2 gap-4">
          <div className="relative aspect-[4/3] bg-[#DEE2E6] rounded-2xl overflow-hidden">
            <img
              src={item.gallery[1] || item.image}
              alt="Gallery 1"
              className="w-full h-full object-cover"
            />
          </div>
          <div className="relative aspect-[4/3] bg-[#DEE2E6] rounded-2xl overflow-hidden">
            <img
              src={item.gallery[2] || item.image}
              alt="Gallery 2"
              className="w-full h-full object-cover"
            />
          </div>
          <div className="relative aspect-[4/3] bg-[#DEE2E6] rounded-2xl overflow-hidden">
            <img
              src={item.gallery[3] || item.image}
              alt="Gallery 3"
              className="w-full h-full object-cover"
            />
          </div>
          {/* 4th box with +2 More Photos overlay */}
          <div className="relative aspect-[4/3] bg-[#DEE2E6] rounded-2xl overflow-hidden group cursor-pointer">
            <img
              src={item.gallery[4] || item.image}
              alt="Gallery 4"
              className="w-full h-full object-cover"
            />
            <div className="absolute inset-0 bg-[#A6AEB8]/80 backdrop-blur-[2px] flex items-center justify-center transition-all group-hover:bg-[#8F99A6]/85">
              <span className="text-2xl font-black text-[#26282E] tracking-tight">
                +2 <span className="text-sm font-bold block text-[#3E4249]">More Photos</span>
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* Main Content & Sticky Booking Column Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-12 text-left">
        {/* Left 2-Columns */}
        <div className="lg:col-span-2 space-y-10">
          {/* Title Row matching Screenshot 4 */}
          <div className="flex items-start justify-between border-b border-neutral-200 pb-6">
            <div>
              <h1 className="text-3xl font-extrabold text-[#22252B] tracking-tight">
                {item.title || 'Well Furnished Apartment'}
              </h1>
              <p className="text-sm text-[#7D828E] font-medium mt-1">
                {item.location || '100 Smart Street, LA, USA'}
              </p>
            </div>

            {/* Heart and Share Action Icons */}
            <div className="flex items-center space-x-3 shrink-0">
              <button
                onClick={() => setIsLiked(!isLiked)}
                className="p-2.5 rounded-full border border-neutral-300 hover:border-neutral-500 bg-white text-[#4A4E55] transition-all"
                aria-label="Wishlist"
              >
                <Heart
                  className={`w-5 h-5 ${
                    isLiked ? 'fill-red-500 text-red-500' : ''
                  }`}
                />
              </button>
              <button
                onClick={handleShare}
                className="p-2.5 rounded-full border border-neutral-300 hover:border-neutral-500 bg-white text-[#4A4E55] transition-all relative"
                aria-label="Share"
              >
                <Share2 className="w-5 h-5" />
                {copiedShare && (
                  <span className="absolute -bottom-8 left-1/2 -translate-x-1/2 text-[10px] bg-black text-white px-2 py-0.5 rounded shadow">
                    Copied!
                  </span>
                )}
              </button>
            </div>
          </div>

          {/* 4 Quick Spec Boxes matching Screenshot 4 */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
            <div className="bg-[#EAECEF] rounded-xl p-5 flex flex-col items-center justify-center text-center">
              <Bed className="w-6 h-6 text-[#3F4249] mb-2" />
              <span className="text-xs font-bold text-[#2A2D33]">
                {item.specs.spec1 || '3 Bedrooms'}
              </span>
            </div>

            <div className="bg-[#EAECEF] rounded-xl p-5 flex flex-col items-center justify-center text-center">
              <Bath className="w-6 h-6 text-[#3F4249] mb-2" />
              <span className="text-xs font-bold text-[#2A2D33]">
                {item.specs.spec2 || '2 Bathrooms'}
              </span>
            </div>

            <div className="bg-[#EAECEF] rounded-xl p-5 flex flex-col items-center justify-center text-center">
              <Car className="w-6 h-6 text-[#3F4249] mb-2" />
              <span className="text-xs font-bold text-[#2A2D33]">
                {item.specs.spec3 || '3 Cars/2 Bikes'}
              </span>
            </div>

            <div className="bg-[#EAECEF] rounded-xl p-5 flex flex-col items-center justify-center text-center">
              <Dog className="w-6 h-6 text-[#3F4249] mb-2" />
              <span className="text-xs font-bold text-[#2A2D33]">
                {item.specs.spec4 || '0 Pets Allowed'}
              </span>
            </div>
          </div>

          {/* Apartment Description matching Screenshot 4 */}
          <div className="space-y-4">
            <h3 className="text-xl font-bold text-[#24272D]">
              Apartment Description
            </h3>
            <p className="text-sm text-[#666B75] leading-relaxed">
              {item.description ||
                'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.'}
            </p>
            <p className="text-sm text-[#666B75] leading-relaxed">
              {item.extendedDescription ||
                'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.'}
            </p>
          </div>

          {/* Offered Amenities matching Screenshot 4 */}
          <div className="space-y-6 pt-4 border-t border-neutral-200">
            <h3 className="text-xl font-bold text-[#24272D]">
              Offered Amenities
            </h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-y-5 gap-x-8 text-sm font-semibold text-[#3D4046]">
              <div className="flex items-center space-x-3.5">
                <Utensils className="w-5 h-5 text-[#5A5E66]" />
                <span>Kitchen</span>
              </div>
              <div className="flex items-center space-x-3.5">
                <Tv className="w-5 h-5 text-[#5A5E66]" />
                <span>Television with Netflix</span>
              </div>
              <div className="flex items-center space-x-3.5">
                <Wind className="w-5 h-5 text-[#5A5E66]" />
                <span>Air Conditioner</span>
              </div>
              <div className="flex items-center space-x-3.5">
                <Wifi className="w-5 h-5 text-[#5A5E66]" />
                <span>Free Wireless Internet</span>
              </div>
              <div className="flex items-center space-x-3.5">
                <Disc className="w-5 h-5 text-[#5A5E66]" />
                <span>Washer</span>
              </div>
              <div className="flex items-center space-x-3.5">
                <Home className="w-5 h-5 text-[#5A5E66]" />
                <span>Balcony or Patio</span>
              </div>
            </div>
          </div>
        </div>

        {/* Right Sticky Booking / Purchase Card matching Screenshot 4 */}
        <div className="lg:col-span-1">
          <div className="sticky top-28 bg-white rounded-3xl border border-neutral-200 shadow-[0_8px_30px_rgb(0,0,0,0.06)] p-8 space-y-6 text-left">
            {/* Price Header */}
            <div className="border-b border-neutral-200 pb-5">
              <h2 className="text-2xl font-black text-[#26282E]">
                $ 1000 - $ 2000
              </h2>
            </div>

            {/* Pricing Period Options */}
            <div className="space-y-3 text-sm font-medium text-[#565A63]">
              {item.pricingTiers?.map((tier, idx) => (
                <div
                  key={tier.period}
                  onClick={() => setSelectedTierIndex(idx)}
                  className={`flex items-center justify-between p-3 rounded-xl border cursor-pointer transition-all ${
                    selectedTierIndex === idx
                      ? 'border-[#26282E] bg-neutral-50/80 text-[#1E1E1E]'
                      : 'border-neutral-100 hover:border-neutral-300'
                  }`}
                >
                  <span>{tier.period}:</span>
                  <span className="font-bold text-[#23262B]">
                    ${tier.price}
                  </span>
                </div>
              )) || (
                <>
                  <div className="flex items-center justify-between">
                    <span>Short Period:</span>
                    <span className="font-bold text-[#23262B]">$ 1000</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span>Medium Period:</span>
                    <span className="font-bold text-[#23262B]">$ 2000</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span>Long Period:</span>
                    <span className="font-bold text-[#23262B]">$ 2000</span>
                  </div>
                </>
              )}
            </div>

            {/* Dark Reserve Button matching Screenshot 4 */}
            <button
              onClick={handleReserve}
              className="w-full bg-[#404349] hover:bg-[#2C2E33] active:scale-98 text-white font-semibold py-3.5 rounded-full transition-all shadow-sm flex items-center justify-center gap-2"
            >
              {reservedSuccess ? (
                <>
                  <Check className="w-5 h-5 text-emerald-400" />
                  <span>Added to Cart!</span>
                </>
              ) : (
                <span>Reserve Now</span>
              )}
            </button>

            {/* Property Inquiry & Contact Host links */}
            <div className="space-y-3.5 pt-2 text-xs font-semibold text-[#3D4046]">
              <button
                onClick={() => onOpenInquiry(item)}
                className="w-full flex items-center gap-3 hover:text-black transition-colors"
              >
                <Building className="w-4 h-4 text-[#5A5E66]" />
                <span>Property Inquiry</span>
              </button>

              <button
                onClick={() => {
                  alert(
                    'Direct Host Desk: +91 98400 12345 or concierge@pyrocraft.com'
                  );
                }}
                className="w-full flex items-center gap-3 hover:text-black transition-colors"
              >
                <Phone className="w-4 h-4 text-[#5A5E66]" />
                <span>Contact Host</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
