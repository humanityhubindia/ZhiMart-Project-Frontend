import React from 'react';
import { ProductCard } from './ProductCard';
import { CrackerProduct, CartItem } from '../types';

interface ProductGridProps {
  title: string;
  products: CrackerProduct[];
  cartItems: CartItem[];
  onAddToCart: (product: CrackerProduct) => void;
  onUpdateQuantity: (product: CrackerProduct, newQty: number) => void;
  onOpenDetail?: (product: CrackerProduct) => void;
}

export const ProductGrid: React.FC<ProductGridProps> = ({
  title,
  products,
  cartItems,
  onAddToCart,
  onUpdateQuantity,
  onOpenDetail,
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
        <span className="text-xs font-semibold text-slate-500">
          Showing {products.length} crackers
        </span>
      </div>

      {/* 4 Cards Per Row Grid matching Screenshot 1 & 2 */}
      {products.length === 0 ? (
        <div className="py-20 text-center bg-white rounded-2xl border border-neutral-200">
          <p className="text-sm font-semibold text-slate-600">
            No products found matching your selection.
          </p>
        </div>
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
