import React from 'react';
import { cn } from '@/lib/utils';
import { CrackerProduct } from '../types';

/** Threshold under which "Only N left" is shown. */
export const LOW_STOCK_THRESHOLD = 10;

export const isLowStock = (product: Pick<CrackerProduct, 'inStock' | 'availableQuantity'>) =>
  product.inStock && product.availableQuantity > 0 && product.availableQuantity <= LOW_STOCK_THRESHOLD;

interface StockBadgeProps {
  product: Pick<CrackerProduct, 'inStock' | 'availableQuantity'>;
  className?: string;
  /** Render nothing for comfortably in-stock products (default true). */
  hideWhenPlenty?: boolean;
}

/** "Out of stock" (slate/rose) or "Only N left" (amber) badge. */
export const StockBadge: React.FC<StockBadgeProps> = ({ product, className, hideWhenPlenty = true }) => {
  if (!product.inStock || product.availableQuantity <= 0) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1 bg-slate-800 text-rose-100 text-[11px] font-black tracking-wide uppercase px-2.5 py-1 rounded-md shadow-xs ring-1 ring-rose-400/40',
          className
        )}
      >
        <span className="w-1.5 h-1.5 rounded-full bg-rose-400" />
        Out of stock
      </span>
    );
  }
  if (isLowStock(product)) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1 bg-amber-50 text-amber-800 border border-amber-200 text-[11px] font-extrabold px-2.5 py-1 rounded-md',
          className
        )}
      >
        Only {product.availableQuantity} left
      </span>
    );
  }
  if (hideWhenPlenty) return null;
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 bg-emerald-50 text-emerald-700 border border-emerald-200 text-[11px] font-extrabold px-2.5 py-1 rounded-md',
        className
      )}
    >
      In stock
    </span>
  );
};
