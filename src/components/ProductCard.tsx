import * as React from 'react';
import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { ChevronLeft, ChevronRight, ArrowRight, Minus, Plus, Package } from 'lucide-react';
import { usePricingRules, useTestMode } from '../hooks/useProducts';
import { TestPriceTag } from './TestModeBanner';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { CrackerProduct } from '../types';
import { StockBadge } from './StockBadge';
import { formatRupees, PricingRules } from '../lib/money';

/**
 * Two-tier pricing helpers shared by the product card, page and modal.
 * `price` is the bulk price (orders reaching `rules.bulkMinOrder`); `standardPrice` applies below it.
 */
export const hasTierPricing = (product: CrackerProduct, testMode: boolean) =>
  !testMode && product.standardPrice > product.price;

/** "₹10,000" */
export const bulkThresholdLabel = (rules: PricingRules) => `₹${formatRupees(rules.bulkMinOrder)}`;

/** "85% OFF · orders ₹10,000+" */
export const bulkPriceLabel = (product: CrackerProduct, rules: PricingRules) =>
  `${product.discountPercent}% OFF · orders ${bulkThresholdLabel(rules)}+`;

interface ProductCardProps {
  product: CrackerProduct;
  quantityInCart: number;
  onAddToCart: (product: CrackerProduct) => void;
  onUpdateQuantity: (product: CrackerProduct, newQty: number) => void;
  onOpenDetail?: (product: CrackerProduct) => void;
  className?: string;
}

export const ProductCard = ({
  product,
  quantityInCart,
  onAddToCart,
  onUpdateQuantity,
  onOpenDetail,
  className,
}: ProductCardProps) => {
  const testMode = useTestMode();
  const rules = usePricingRules();
  const tiered = hasTierPricing(product, testMode);
  const images = Array.from(new Set([product.image, ...(product.gallery ?? [])].filter(Boolean)));
  const hasCarousel = images.length > 1;
  const isOutOfStock = !product.inStock || product.availableQuantity <= 0;
  const canIncrease = !isOutOfStock && quantityInCart < product.availableQuantity;

  const [currentIndex, setCurrentIndex] = useState(0);
  const [direction, setDirection] = useState(0);

  // Carousel image change handler
  const changeImage = (newDirection: number, e: React.MouseEvent) => {
    e.stopPropagation();
    setDirection(newDirection);
    setCurrentIndex((prevIndex) => {
      const nextIndex = prevIndex + newDirection;
      if (nextIndex < 0) return images.length - 1;
      if (nextIndex >= images.length) return 0;
      return nextIndex;
    });
  };

  // Carousel animation variants
  const carouselVariants = {
    enter: (direction: number) => ({
      x: direction > 0 ? '100%' : '-100%',
      opacity: 0,
    }),
    center: {
      zIndex: 1,
      x: 0,
      opacity: 1,
    },
    exit: (direction: number) => ({
      zIndex: 0,
      x: direction < 0 ? '100%' : '-100%',
      opacity: 0,
    }),
  };

  return (
    <div
      className={cn(
        'w-full bg-white rounded-2xl border border-slate-200/90 hover:border-slate-300 hover:shadow-xl transition-all duration-300 flex flex-col justify-between text-left group overflow-hidden',
        className
      )}
    >
      {/* 1. Image Carousel Container with Generous Breathing Room */}
      <div 
        onClick={() => onOpenDetail?.(product)}
        className="relative h-60 bg-[#FAFBFD] flex items-center justify-center cursor-pointer p-6 overflow-hidden border-b border-slate-100 select-none"
      >
        <AnimatePresence initial={false} custom={direction}>
          <motion.img
            key={currentIndex}
            src={images[Math.min(currentIndex, images.length - 1)]}
            alt={product.name}
            custom={direction}
            variants={carouselVariants}
            initial="enter"
            animate="center"
            exit="exit"
            transition={{
              x: { type: 'spring', stiffness: 300, damping: 30 },
              opacity: { duration: 0.2 },
            }}
            className={cn(
              'absolute max-h-44 max-w-[85%] object-contain drop-shadow-sm transition-transform duration-300 group-hover:scale-105',
              isOutOfStock && 'opacity-60 grayscale'
            )}
          />
        </AnimatePresence>

        {/* Top Badges: discount + stock */}
        <div className="absolute top-3.5 left-3.5 right-3.5 z-20 flex flex-wrap items-start justify-between gap-2">
          {testMode ? (
            <TestPriceTag />
          ) : product.discountPercent > 0 ? (
            <span className="bg-[#7C2D12] text-white text-[11px] font-black tracking-wide uppercase px-2.5 py-1 rounded-md shadow-xs">
              {tiered ? `Up to ${product.discountPercent}% OFF` : `${product.discountPercent}% OFF`}
            </span>
          ) : product.originalPrice > 0 && product.originalPrice <= product.price ? (
            <span className="bg-slate-900 text-amber-300 text-[11px] font-black tracking-wide uppercase px-2.5 py-1 rounded-md shadow-xs">
              Fixed rate
            </span>
          ) : (
            <span />
          )}
          <StockBadge product={product} />
        </div>

        {/* Carousel Navigation Chevrons on Hover */}
        {hasCarousel && (
        <div className="absolute inset-0 flex items-center justify-between px-2.5 opacity-0 group-hover:opacity-100 transition-opacity z-20 pointer-events-none">
          <Button 
            variant="ghost" 
            size="icon" 
            className="rounded-full bg-black/40 hover:bg-black/70 text-white w-8 h-8 pointer-events-auto shadow-sm" 
            onClick={(e) => changeImage(-1, e)}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button 
            variant="ghost" 
            size="icon" 
            className="rounded-full bg-black/40 hover:bg-black/70 text-white w-8 h-8 pointer-events-auto shadow-sm" 
            onClick={(e) => changeImage(1, e)}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
        )}

        {/* Pagination Dots */}
        {hasCarousel && (
        <div className="absolute bottom-3 left-1/2 -translate-x-1/2 flex items-center gap-1.5 z-20">
          {images.map((_, index) => (
            <button
              key={index}
              onClick={(e) => {
                e.stopPropagation();
                setCurrentIndex(index);
              }}
              className={cn(
                'h-1.5 rounded-full transition-all',
                currentIndex === index ? 'w-4 bg-slate-800' : 'w-1.5 bg-slate-300 hover:bg-slate-400'
              )}
              aria-label={`Slide ${index + 1}`}
            />
          ))}
        </div>
        )}
      </div>

      {/* 2. Body Content with Clear Hierarchy */}
      <div className="p-4 sm:p-5 flex-1 flex flex-col justify-between space-y-3">
        <div>
          {/* Product Title on top */}
          <h3 
            onClick={() => onOpenDetail?.(product)}
            title={product.name}
            className="text-base sm:text-lg font-bold text-slate-900 leading-snug cursor-pointer hover:text-amber-600 transition-colors line-clamp-2"
          >
            {product.name}
          </h3>

          {/* Category subtitle below name */}
          <p className="text-[11px] font-bold text-slate-400 uppercase tracking-wider mt-1">
            {product.category}
          </p>

          {/* Packaging tag below name and category */}
          <div className="pt-2">
            <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-slate-600 bg-slate-100/90 px-2.5 py-1 rounded-lg">
              <Package className="w-3.5 h-3.5 text-amber-700" />
              <span>{product.packInfo}</span>
            </span>
          </div>
        </div>

        {/* 3. Pricing and Action Button with Ample Breathing Space */}
        <div className="pt-4 border-t border-slate-100 space-y-2.5">
          <div className="flex flex-wrap items-end justify-between gap-3">
            {/* Price block: bulk (price-list) price; the standard price is shown underneath */}
            <div className="min-w-0">
              <div className="flex flex-wrap items-baseline gap-x-1.5">
                <span className="text-xl font-extrabold text-slate-900 tracking-tight">
                  ₹{formatRupees(product.price)}
                </span>
                {!testMode && product.originalPrice > product.price && (
                  <span className="text-xs text-slate-400 line-through font-medium">
                    ₹{formatRupees(product.originalPrice)}
                  </span>
                )}
              </div>
              {tiered ? (
                <span className="text-[11px] font-bold text-emerald-700 block mt-0.5 leading-tight">
                  {bulkPriceLabel(product, rules)}
                </span>
              ) : (
                !testMode &&
                product.originalPrice > product.price && (
                  <span className="text-[11px] font-bold text-emerald-600 block mt-0.5">
                    Save ₹{formatRupees(product.originalPrice - product.price)}
                  </span>
                )
              )}
            </div>

            {/* Action Button: Amber Add to Cart or Dark - 1 + Stepper */}
            <div className="shrink-0">
              {quantityInCart > 0 ? (
                <div className="bg-[#0F172A] text-white flex items-center space-x-2 px-3 py-2 rounded-xl font-bold text-xs shadow-sm select-none">
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      onUpdateQuantity(product, quantityInCart - 1);
                    }}
                    className="p-1 hover:text-amber-400 active:scale-90 transition-transform"
                    aria-label="Decrease"
                  >
                    <Minus className="w-3.5 h-3.5" />
                  </button>
                  <span className="px-1 text-xs font-bold">{quantityInCart}</span>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      onUpdateQuantity(product, quantityInCart + 1);
                    }}
                    disabled={!canIncrease}
                    title={canIncrease ? undefined : 'No more stock available'}
                    className="p-1 hover:text-amber-400 active:scale-90 transition-transform disabled:opacity-30 disabled:hover:text-white disabled:active:scale-100 disabled:cursor-not-allowed"
                    aria-label="Increase"
                  >
                    <Plus className="w-3.5 h-3.5" />
                  </button>
                </div>
              ) : isOutOfStock ? (
                <Button
                  disabled
                  aria-disabled="true"
                  className="bg-slate-100 text-slate-500 font-bold text-xs px-4 py-2.5 h-auto rounded-xl border border-slate-200 cursor-not-allowed"
                >
                  Out of stock
                </Button>
              ) : (
                <Button 
                  onClick={(e) => {
                    e.stopPropagation();
                    onAddToCart(product);
                  }}
                  className="bg-[#F59E0B] hover:bg-[#D97706] active:scale-96 text-[#0F172A] font-bold text-xs px-4 py-2.5 h-auto rounded-xl group shadow-xs transition-all flex items-center gap-1.5"
                >
                  <span>Add to Cart</span>
                  <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-1" />
                </Button>
              )}
            </div>
          </div>
          {tiered && (
            <p className="text-[11px] leading-snug text-slate-600 bg-slate-50 border border-slate-200/80 rounded-lg px-2.5 py-1.5">
              Below {bulkThresholdLabel(rules)} order:{' '}
              <span className="font-extrabold text-slate-900">₹{formatRupees(product.standardPrice)}</span>{' '}
              <span className="whitespace-nowrap">({product.standardDiscountPercent}% OFF)</span>
            </p>
          )}
        </div>
      </div>
    </div>
  );
};
