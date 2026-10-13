import React from 'react';
import { ProductCard } from './ProductCard';
import { CrackerProduct, CartItem } from '../types';
import { CatalogErrorCard, EmptyCatalogState, ProductGridSkeleton } from './CatalogStates';

interface ProductGridProps {
  title: string;
  products: CrackerProduct[];
  cartItems: CartItem[];
  onAddToCart: (product: CrackerProduct) => void;
  onUpdateQuantity: (product: CrackerProduct, newQty: number) => void;
  onOpenDetail?: (product: CrackerProduct) => void;
  isLoading?: boolean;
  /** Error to show when the catalog could not be loaded (and there is no data to show). */
  error?: string | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export const ProductGrid: React.FC<ProductGridProps> = ({
  title,
  products,
  cartItems,
  onAddToCart,
  onUpdateQuantity,
  onOpenDetail,
  isLoading,
  error,
  onRetry,
  isRetrying,
}) => {
  const getProductQuantity = (productId: string) => {
    const item = cartItems.find((ci) => ci.product.id === productId);
    return item ? item.quantity : 0;
  };

  return (
    <section id="products-section" className="max-w-7xl mx-auto px-4 sm:px-6 py-8">
      {/* Section Title matching Screenshot 1 */}
      <div className="flex items-center justify-between mb-6 text-left">
        <h2 className="text-xl sm:text-2xl font-bold text-[#0F172A] tracking-tight">
          {title}
        </h2>
        {!isLoading && !error && (
          <span className="text-xs font-semibold text-slate-500">
            Showing {products.length} crackers
          </span>
        )}
      </div>

      {/* 4 Cards Per Row Grid matching Screenshot 1 & 2 */}
      {isLoading ? (
        <ProductGridSkeleton />
      ) : error ? (
        <CatalogErrorCard message={error} onRetry={onRetry} isRetrying={isRetrying} />
      ) : products.length === 0 ? (
        <EmptyCatalogState />
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6">
          {products.map((product) => (
            <ProductCard
              key={product.id}
              product={product}
              quantityInCart={getProductQuantity(product.id)}
              onAddToCart={onAddToCart}
              onUpdateQuantity={onUpdateQuantity}
              onOpenDetail={onOpenDetail}
            />
          ))}
        </div>
      )}
    </section>
  );
};
