import React from 'react';
import { X, Package, ShieldCheck, Volume2, Plus, Minus, ShoppingBag } from 'lucide-react';
import { CrackerProduct } from '../types';

interface ProductDetailModalProps {
  product: CrackerProduct | null;
  quantityInCart: number;
  onClose: () => void;
  onAddToCart: (product: CrackerProduct) => void;
  onUpdateQuantity: (product: CrackerProduct, newQty: number) => void;
}

export const ProductDetailModal: React.FC<ProductDetailModalProps> = ({
  product,
  quantityInCart,
  onClose,
  onAddToCart,
  onUpdateQuantity,
}) => {
  if (!product) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-2xl bg-white rounded-3xl shadow-2xl border border-slate-200 overflow-hidden text-left animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top Header */}
        <div className="p-5 border-b border-neutral-100 flex items-center justify-between">
          <span className="bg-[#78281F] text-white text-[11px] font-black uppercase px-2.5 py-0.5 rounded">
            {product.discountPercent}% FESTIVAL DISCOUNT
          </span>
          <button
            onClick={onClose}
            className="p-1.5 rounded-full hover:bg-neutral-100 text-slate-500"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content Body */}
        <div className="p-6 grid grid-cols-1 md:grid-cols-2 gap-8 items-center">
          {/* Image */}
          <div className="w-full h-64 bg-slate-50 rounded-2xl flex items-center justify-center p-4 border border-neutral-100">
            <img
              src={product.image}
              alt={product.name}
              className="max-h-full max-w-full object-contain"
            />
          </div>

          {/* Specs */}
          <div className="space-y-4">
            <div>
              <p className="text-[11px] font-bold text-[#64748B] uppercase tracking-wider">
                {product.category}
              </p>
              <h2 className="text-2xl font-black text-[#0F172A] leading-tight">
                {product.name}
              </h2>
              <p className="text-xs font-semibold text-amber-700 mt-1">
                By {product.brand}
              </p>
            </div>

            <div className="flex items-baseline space-x-2">
              <span className="text-2xl font-black text-[#0F172A]">
                ₹{product.price}
              </span>
              <span className="text-sm text-slate-400 line-through">
                ₹{product.originalPrice}
              </span>
              <span className="text-xs text-emerald-600 font-bold ml-2">
                Save ₹{product.originalPrice - product.price}
              </span>
            </div>

            <p className="text-xs text-slate-600 leading-relaxed">
              {product.description}
            </p>

            <div className="grid grid-cols-2 gap-2 text-xs font-semibold text-slate-700 pt-2 border-t border-neutral-100">
              <div className="flex items-center gap-2">
                <Package className="w-4 h-4 text-amber-600" />
                <span>{product.packInfo}</span>
              </div>
              <div className="flex items-center gap-2">
                <Volume2 className="w-4 h-4 text-blue-600" />
                <span>{product.soundLevel || 'Tested Decibel Safe'}</span>
              </div>
              <div className="flex items-center gap-2 col-span-2 text-emerald-700">
                <ShieldCheck className="w-4 h-4 text-emerald-600" />
                <span>Govt Authorized Sivakasi Dispatch</span>
              </div>
            </div>

            {/* Actions */}
            <div className="pt-4">
              {quantityInCart > 0 ? (
                <div className="w-full bg-[#0F172A] text-white flex items-center justify-between px-6 py-3 rounded-xl font-bold text-sm shadow-sm">
                  <button
                    type="button"
                    onClick={() => onUpdateQuantity(product, quantityInCart - 1)}
                    className="p-1 hover:text-amber-400"
                  >
                    <Minus className="w-4 h-4" />
                  </button>
                  <span className="text-base">{quantityInCart} in Cart</span>
                  <button
                    type="button"
                    onClick={() => onUpdateQuantity(product, quantityInCart + 1)}
                    className="p-1 hover:text-amber-400"
                  >
                    <Plus className="w-4 h-4" />
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => onAddToCart(product)}
                  className="w-full bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] font-bold py-3.5 rounded-xl transition-all shadow-md text-sm flex items-center justify-center gap-2"
                >
                  <ShoppingBag className="w-4 h-4" />
                  <span>Add to Cart (₹{product.price})</span>
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
