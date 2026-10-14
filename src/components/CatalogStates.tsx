import React from 'react';
import { AlertTriangle, ArrowLeft, PackageX, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/utils';

/**
 * Skeleton matching ProductCard's layout box-for-box (same paddings, line boxes and wrapping), so
 * the grid doesn't shift when products arrive. The price block uses invisible "ghost" text with the
 * card's font sizes for the tier label, the Add-to-Cart button and the "Below ₹10,000 order" box,
 * so those wrap at exactly the same widths as the real card (button under the price on narrow
 * columns, two-line box on tablet columns). Keep in sync with ProductCard.tsx.
 */
export const ProductCardSkeleton: React.FC = () => (
  <div
    aria-hidden="true"
    className="w-full bg-white rounded-2xl border border-slate-200/90 overflow-hidden animate-pulse flex flex-col select-none"
  >
    {/* Image area + discount badge */}
    <div className="relative h-60 bg-[#FAFBFD] border-b border-slate-100 flex items-center justify-center p-6">
      <div className="absolute top-3.5 left-3.5 h-6 w-24 bg-slate-200/80 rounded-md" />
      <div className="absolute top-3.5 right-3.5 h-6 w-16 bg-slate-100 rounded-full" />
      <div className="h-36 w-1/2 bg-slate-100 rounded-2xl" />
    </div>

    <div className="p-4 sm:p-5 flex-1 flex flex-col justify-between space-y-3">
      <div>
        {/* Title (text-base/leading-snug → 22px; sm:text-lg → 28px line). Most names wrap to two
            lines in the narrow 3/4-column cards (md…xl), so show a second line there. */}
        <div className="h-[22px] sm:h-7 flex items-center">
          <div className="h-4 sm:h-5 w-3/4 bg-slate-200 rounded-md" />
        </div>
        <div className="hidden md:flex xl:hidden h-7 items-center">
          <div className="h-5 w-1/2 bg-slate-200 rounded-md" />
        </div>
        {/* Category (11px text → 16.5px line) */}
        <div className="mt-1 h-[16.5px] flex items-center">
          <div className="h-2.5 w-20 bg-slate-100 rounded" />
        </div>
        {/* Pack tag (inline pill → 25.5px line box) */}
        <div className="pt-2 h-[33.5px] flex items-end">
          <div className="h-6 w-24 bg-slate-100 rounded-lg" />
        </div>
      </div>

      {/* Price block: big price + tier label, Add-to-Cart button, "Below ₹10,000 order" box */}
      <div className="pt-4 border-t border-slate-100 space-y-2.5">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div className="min-w-0">
            <div className="h-7 flex items-center">
              <div className="h-6 w-24 bg-slate-200 rounded-md" />
            </div>
            <span className="block mt-0.5 text-[11px] font-bold leading-tight text-transparent bg-slate-100 rounded">
              85% OFF · orders ₹10,000+
            </span>
          </div>
          <div className="shrink-0 inline-flex items-center gap-1.5 px-4 py-2.5 rounded-xl bg-amber-100 text-xs font-bold text-transparent">
            <span>Add to Cart</span>
            <span className="w-3.5 h-3.5" />
          </div>
        </div>
        <p className="text-[11px] leading-snug text-transparent bg-slate-50 border border-slate-200/80 rounded-lg px-2.5 py-1.5">
          Below ₹10,000 order: <span className="font-extrabold">₹100</span>{' '}
          <span className="whitespace-nowrap">(75% OFF)</span>
        </p>
      </div>
    </div>
  </div>
);

export const ProductGridSkeleton: React.FC<{ count?: number }> = ({ count = 8 }) => (
  <div
    className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6"
    aria-busy="true"
    aria-label="Loading products"
  >
    {Array.from({ length: count }, (_, i) => (
      <ProductCardSkeleton key={i} />
    ))}
  </div>
);

/** Skeleton rows for list/table style views (quick estimate, AI assistant). */
export const ListSkeleton: React.FC<{ rows?: number; className?: string }> = ({ rows = 6, className }) => (
  <div className={cn('divide-y divide-slate-100 animate-pulse', className)} aria-busy="true">
    {Array.from({ length: rows }, (_, i) => (
      <div key={i} className="py-4 px-4 flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-slate-100 shrink-0" />
        <div className="flex-1 space-y-1.5">
          <div className="h-3.5 w-1/2 bg-slate-200 rounded" />
          <div className="h-2.5 w-1/3 bg-slate-100 rounded" />
        </div>
        <div className="h-7 w-24 bg-slate-100 rounded-xl" />
      </div>
    ))}
  </div>
);

export const ProductPageSkeleton: React.FC = () => (
  <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8 animate-pulse" aria-busy="true">
    <div className="h-10 w-48 bg-slate-100 rounded-xl" />
    <div className="grid grid-cols-1 lg:grid-cols-12 gap-10 pt-8">
      <div className="lg:col-span-5 aspect-square bg-slate-100 rounded-3xl" />
      <div className="lg:col-span-7 space-y-4">
        <div className="h-3 w-40 bg-slate-100 rounded" />
        <div className="h-10 w-3/4 bg-slate-200 rounded-lg" />
        <div className="h-16 w-full bg-slate-100 rounded-2xl" />
        <div className="h-24 w-full bg-slate-100 rounded-2xl" />
        <div className="h-12 w-56 bg-amber-100 rounded-2xl" />
      </div>
    </div>
  </div>
);

interface CatalogErrorCardProps {
  title?: string;
  message?: string;
  onRetry?: () => void;
  isRetrying?: boolean;
  className?: string;
}

export const CatalogErrorCard: React.FC<CatalogErrorCardProps> = ({
  title = "We couldn't load the catalog",
  message = 'Please check your connection and try again.',
  onRetry,
  isRetrying,
  className,
}) => (
  <div
    role="alert"
    className={cn(
      'py-14 px-6 text-center bg-white rounded-2xl border border-rose-200 space-y-4',
      className
    )}
  >
    <div className="w-14 h-14 rounded-2xl bg-rose-50 text-rose-600 flex items-center justify-center mx-auto">
      <AlertTriangle className="w-7 h-7" />
    </div>
    <div className="space-y-1">
      <h3 className="text-base font-black text-slate-900">{title}</h3>
      <p className="text-xs text-slate-500 max-w-sm mx-auto">{message}</p>
    </div>
    {onRetry && (
      <button
        type="button"
        onClick={onRetry}
        disabled={isRetrying}
        className="inline-flex items-center gap-2 bg-[#0F172A] hover:bg-slate-800 disabled:opacity-60 text-white text-xs font-extrabold px-5 py-2.5 rounded-xl transition-all shadow-md active:scale-97"
      >
        <RefreshCw className={cn('w-4 h-4 text-amber-400', isRetrying && 'animate-spin')} />
        <span>{isRetrying ? 'Retrying…' : 'Retry'}</span>
      </button>
    )}
  </div>
);

export const EmptyCatalogState: React.FC<{ message?: string; className?: string }> = ({
  message = 'No products found matching your selection.',
  className,
}) => (
  <div className={cn('py-20 text-center bg-white rounded-2xl border border-neutral-200', className)}>
    <PackageX className="w-8 h-8 text-slate-300 mx-auto mb-3" />
    <p className="text-sm font-semibold text-slate-600">{message}</p>
  </div>
);

export const ProductNotFound: React.FC<{ productId?: string; onBack: () => void }> = ({ productId, onBack }) => (
  <div className="max-w-2xl mx-auto px-4 sm:px-6 py-20 text-center space-y-6">
    <div className="w-20 h-20 rounded-3xl bg-slate-100 text-slate-400 flex items-center justify-center mx-auto shadow-inner">
      <PackageX className="w-10 h-10" />
    </div>
    <div className="space-y-2">
      <h1 className="text-2xl font-black text-slate-900 font-heading">Product not found</h1>
      <p className="text-sm text-slate-500 leading-relaxed">
        {productId ? (
          <>
            We couldn't find a product with id <strong className="font-mono text-slate-700">{productId}</strong>.
          </>
        ) : (
          "We couldn't find that product."
        )}{' '}
        It may have been removed from the catalog.
      </p>
    </div>
    <button
      type="button"
      onClick={onBack}
      className="inline-flex items-center gap-2 bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] font-extrabold text-sm px-6 py-3.5 rounded-2xl transition-all shadow-md active:scale-97"
    >
      <ArrowLeft className="w-4 h-4" />
      <span>Back to All Crackers</span>
    </button>
  </div>
);

/** Readable message from a react-query error. */
export const errorMessage = (error: unknown, fallback = 'Please check your connection and try again.') =>
  error instanceof Error && error.message ? error.message : fallback;
