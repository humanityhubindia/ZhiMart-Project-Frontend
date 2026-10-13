import React, { useState } from 'react';
import {
  ArrowLeft,
  Share2,
  Check,
  Package,
  Volume2,
  ShieldCheck,
  Truck,
  Minus,
  Plus,
  ShoppingBag,
  ExternalLink,
  MessageCircle,
} from 'lucide-react';
import { usePricingRules, useTestMode } from '../hooks/useProducts';
import { bulkPriceLabel, bulkThresholdLabel, hasTierPricing } from './ProductCard';
import { TestPriceTag } from './TestModeBanner';
import { CrackerProduct } from '../types';
import { StockBadge } from './StockBadge';
import { formatRupees } from '../lib/money';

interface ProductPageProps {
  product: CrackerProduct;
  quantityInCart: number;
  onBack: () => void;
  onAddToCart: (product: CrackerProduct) => void;
  onUpdateQuantity: (product: CrackerProduct, newQty: number) => void;
  onGoToCart?: () => void;
}

export const ProductPage: React.FC<ProductPageProps> = ({
  product,
  quantityInCart,
  onBack,
  onAddToCart,
  onUpdateQuantity,
  onGoToCart,
}) => {
  const testMode = useTestMode();
  const rules = usePricingRules();
  const tiered = hasTierPricing(product, testMode);
  const images = Array.from(new Set([product.image, ...(product.gallery ?? [])].filter(Boolean)));
  const isOutOfStock = !product.inStock || product.availableQuantity <= 0;
  const canIncrease = !isOutOfStock && quantityInCart < product.availableQuantity;

  const [activeImageIndex, setActiveImageIndex] = useState(0);
  const [copiedLink, setCopiedLink] = useState(false);

  // Generate shareable URL with product ID
  const shareableUrl = `${window.location.origin}${window.location.pathname}?product=${product.id}`;

  const handleShareClick = async () => {
    try {
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(shareableUrl);
        setCopiedLink(true);
        setTimeout(() => setCopiedLink(false), 2500);
      }
    } catch (err) {
      // Fallback
      setCopiedLink(true);
      setTimeout(() => setCopiedLink(false), 2500);
    }
  };

  const handleWhatsAppShare = () => {
    const text = encodeURIComponent(
      `Check out ${product.name} at Zhi-mart Crackers ${tiered ? 'from' : 'for'} ₹${formatRupees(product.price)}${
        !testMode && product.discountPercent > 0
          ? tiered
            ? ` (${bulkPriceLabel(product, rules)})`
            : ` (${product.discountPercent}% OFF)`
          : ''
      }!\n${shareableUrl}`
    );
    window.open(`https://api.whatsapp.com/send?text=${text}`, '_blank');
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8 animate-in fade-in duration-200 text-left">
      {/* 1. Top Navigation & Breadcrumbs */}
      <div className="flex flex-wrap items-center justify-between gap-4 pb-6 border-b border-slate-200">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-2 text-sm font-bold text-slate-700 hover:text-slate-900 bg-white border border-slate-200 px-4 py-2 rounded-xl transition-all shadow-2xs hover:bg-slate-50 active:scale-97"
        >
          <ArrowLeft className="w-4 h-4" />
          <span>Back to All Crackers</span>
        </button>

        <div className="flex items-center gap-2">
          {/* Share Button with Direct Link Copy */}
          <button
            onClick={handleShareClick}
            className="inline-flex items-center gap-2 text-xs font-bold px-4 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-800 transition-all shadow-2xs active:scale-97"
            title="Copy shareable link"
          >
            {copiedLink ? (
              <>
                <Check className="w-4 h-4 text-emerald-600" />
                <span className="text-emerald-700 font-bold">Link Copied!</span>
              </>
            ) : (
              <>
                <Share2 className="w-4 h-4 text-slate-600" />
                <span>Share Link</span>
              </>
            )}
          </button>

          {/* WhatsApp Share Button */}
          <button
            onClick={handleWhatsAppShare}
            className="inline-flex items-center gap-1.5 text-xs font-bold px-4 py-2 rounded-xl bg-[#25D366] hover:bg-[#20ba5a] text-white transition-all shadow-2xs active:scale-97"
          >
            <MessageCircle className="w-4 h-4" />
            <span>WhatsApp</span>
          </button>
        </div>
      </div>

      {/* 2. Main Product Layout */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-10 pt-8">
        {/* Left Column: Image Gallery (5 cols) */}
        <div className="lg:col-span-5 space-y-4">
          {/* Main Large Image Container */}
          <div className="relative aspect-square bg-white rounded-3xl border border-slate-200/90 p-8 flex items-center justify-center overflow-hidden shadow-sm">
            {/* Discount Tag */}
            {testMode ? (
              <TestPriceTag className="absolute top-4 left-4" />
            ) : product.discountPercent > 0 && (
              <span className="absolute top-4 left-4 bg-[#7C2D12] text-white text-xs font-black tracking-wide uppercase px-3 py-1 rounded-md shadow-xs">
                {tiered ? `Up to ${product.discountPercent}% OFF` : `${product.discountPercent}% OFF`}
              </span>
            )}
            <StockBadge product={product} className="absolute top-4 right-4" />

            <img
              src={images[activeImageIndex] || product.image}
              alt={product.name}
              className={`max-h-full max-w-full object-contain drop-shadow-md transition-transform duration-300 hover:scale-105 ${
                isOutOfStock ? 'opacity-60 grayscale' : ''
              }`}
            />
          </div>

          {/* Gallery Thumbnails */}
          {images.length > 1 && (
          <div className="flex items-center gap-3">
            {images.map((img, idx) => (
              <button
                key={idx}
                onClick={() => setActiveImageIndex(idx)}
                className={`w-20 h-20 rounded-2xl bg-white border-2 p-2 flex items-center justify-center overflow-hidden transition-all ${
                  activeImageIndex === idx
                    ? 'border-amber-500 shadow-md ring-2 ring-amber-500/20'
                    : 'border-slate-200 hover:border-slate-400'
                }`}
              >
                <img
                  src={img}
                  alt={`Thumbnail ${idx + 1}`}
                  className="max-h-full max-w-full object-contain"
                />
              </button>
            ))}
          </div>
          )}
        </div>

        {/* Right Column: Product Details & Buying Actions (7 cols) */}
        <div className="lg:col-span-7 space-y-6">
          <div>
            <p className="text-xs font-bold text-slate-400 uppercase tracking-widest">
              {product.category}
            </p>
            <h1 className="text-3xl sm:text-4xl font-extrabold text-[#0F172A] tracking-tight mt-1 leading-tight">
              {product.name}
            </h1>
            <p className="text-xs font-semibold text-amber-700 mt-1.5">
              By {product.brand}
            </p>
            <div className="mt-3">
              <StockBadge product={product} hideWhenPlenty={false} />
            </div>
          </div>

          {/* Pricing Row: bulk (price-list) price, plus the standard price below the order threshold */}
          <div className="p-4 bg-slate-50 rounded-2xl border border-slate-200/80 space-y-2.5">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1.5">
              <span className="text-3xl font-black text-slate-900 tracking-tight">
                ₹{formatRupees(product.price)}
              </span>
              {testMode ? (
                <TestPriceTag />
              ) : product.originalPrice > product.price && (
                <>
                  <span className="text-sm font-semibold text-slate-400 line-through">
                    ₹{formatRupees(product.originalPrice)}
                  </span>
                  <span className="text-xs font-bold text-emerald-700 bg-emerald-100/80 px-2.5 py-1 rounded-md">
                    {tiered
                      ? `Save up to ₹${formatRupees(product.originalPrice - product.price)}`
                      : `Save ₹${formatRupees(product.originalPrice - product.price)} (${product.discountPercent}% Festival Savings)`}
                  </span>
                </>
              )}
            </div>
            {tiered && (
              <>
                <p className="text-xs font-bold text-emerald-700">{bulkPriceLabel(product, rules)}</p>
                <div className="pt-2.5 border-t border-slate-200 space-y-1">
                  <p className="text-sm text-slate-700">
                    Below {bulkThresholdLabel(rules)} order:{' '}
                    <span className="font-extrabold text-slate-900">₹{formatRupees(product.standardPrice)}</span>{' '}
                    <span className="text-xs font-bold text-slate-500 whitespace-nowrap">
                      ({product.standardDiscountPercent}% OFF)
                    </span>
                  </p>
                  <p className="text-[11px] text-slate-500 leading-relaxed">
                    Once your cart adds up to {bulkThresholdLabel(rules)} at the {product.discountPercent}% OFF prices,
                    every item in it is billed at the lower price.
                  </p>
                </div>
              </>
            )}
          </div>

          {/* Quick Specifications */}
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-3 text-xs font-semibold text-slate-700">
            <div className="p-3 bg-white border border-slate-200 rounded-xl flex items-center gap-2.5">
              <Package className="w-4 h-4 text-amber-600 shrink-0" />
              <div>
                <span className="text-[10px] text-slate-400 block font-normal">Pack Info</span>
                <span>{product.packInfo}</span>
              </div>
            </div>

            <div className="p-3 bg-white border border-slate-200 rounded-xl flex items-center gap-2.5">
              <Volume2 className="w-4 h-4 text-blue-600 shrink-0" />
              <div>
                <span className="text-[10px] text-slate-400 block font-normal">Sound Level</span>
                <span>{product.soundLevel || 'Tested Decibel Safe'}</span>
              </div>
            </div>

            <div className="p-3 bg-white border border-slate-200 rounded-xl flex items-center gap-2.5">
              <ShieldCheck className="w-4 h-4 text-emerald-600 shrink-0" />
              <div>
                <span className="text-[10px] text-slate-400 block font-normal">Certification</span>
                <span>100% Genuine Sivakasi</span>
              </div>
            </div>
          </div>

          {/* Detailed Description */}
          <div className="space-y-2 pt-2">
            <h3 className="text-sm font-bold text-slate-900 uppercase tracking-wide">
              Product Overview
            </h3>
            <p className="text-sm text-slate-600 leading-relaxed">
              {product.description}
            </p>
          </div>

          {/* Add to Cart / Quantity Actions */}
          <div className="pt-4 border-t border-slate-200 flex flex-wrap items-center gap-4">
            {quantityInCart > 0 ? (
              <div className="flex flex-wrap items-center gap-3">
                <div className="flex items-center space-x-3 bg-[#0F172A] text-white px-5 py-3 rounded-2xl font-bold shadow-md">
                  <button
                    type="button"
                    onClick={() => onUpdateQuantity(product, quantityInCart - 1)}
                    className="p-1.5 hover:text-amber-400 active:scale-90 transition-transform"
                    aria-label="Decrease"
                  >
                    <Minus className="w-4 h-4" />
                  </button>
                  <span className="px-3 text-base">{quantityInCart} in Cart</span>
                  <button
                    type="button"
                    onClick={() => onUpdateQuantity(product, quantityInCart + 1)}
                    disabled={!canIncrease}
                    title={canIncrease ? undefined : 'No more stock available'}
                    className="p-1.5 hover:text-amber-400 active:scale-90 transition-transform disabled:opacity-30 disabled:hover:text-white disabled:cursor-not-allowed"
                    aria-label="Increase"
                  >
                    <Plus className="w-4 h-4" />
                  </button>
                </div>

                {onGoToCart && (
                  <button
                    type="button"
                    onClick={onGoToCart}
                    className="bg-[#F59E0B] hover:bg-[#D97706] active:scale-97 text-[#0F172A] font-extrabold text-sm px-6 py-3.5 rounded-2xl shadow-md transition-all flex items-center gap-2"
                  >
                    <ShoppingBag className="w-4 h-4" />
                    <span>View Cart & Checkout</span>
                  </button>
                )}
              </div>
            ) : isOutOfStock ? (
              <button
                type="button"
                disabled
                className="bg-slate-100 text-slate-500 border border-slate-200 font-extrabold text-sm px-8 py-3.5 rounded-2xl flex items-center gap-2 cursor-not-allowed"
              >
                <ShoppingBag className="w-4 h-4" />
                <span>Out of stock</span>
              </button>
            ) : (
              <button
                type="button"
                onClick={() => onAddToCart(product)}
                className="bg-[#F59E0B] hover:bg-[#D97706] active:scale-97 text-[#0F172A] font-extrabold text-sm px-8 py-3.5 rounded-2xl shadow-md transition-all flex items-center gap-2"
              >
                <ShoppingBag className="w-4 h-4" />
                <span>Add to Cart</span>
              </button>
            )}

            {/* Direct Share Link Box */}
            <div className="flex-1 min-w-[240px] flex items-center gap-2 bg-slate-100 rounded-xl px-3 py-2 border border-slate-200">
              <span className="text-[11px] text-slate-500 truncate select-all flex-1 font-mono">
                {shareableUrl}
              </span>
              <button
                onClick={handleShareClick}
                className="text-xs font-bold text-amber-700 hover:text-amber-800 shrink-0 px-2 py-1 rounded"
              >
                {copiedLink ? 'Copied!' : 'Copy'}
              </button>
            </div>
          </div>

          {/* Transport and Safety Highlights */}
          <div className="p-4 bg-amber-50/60 rounded-2xl border border-amber-200/70 text-xs text-amber-900 flex items-start gap-3">
            <Truck className="w-5 h-5 text-amber-600 shrink-0 mt-0.5" />
            <div className="space-y-1">
              <p className="font-bold">
                Direct Sivakasi Factory Parcel Dispatch
              </p>
              <p className="text-amber-800 leading-relaxed text-[11px]">
                Packed in heavy unbleached safety cartons. Delivered safely to all transport hubs across India with parcel tracking docket.
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
