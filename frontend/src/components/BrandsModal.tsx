import React from 'react';
import { X, Tag } from 'lucide-react';
import { useProducts, countBy } from '../hooks/useProducts';
import { CatalogErrorCard, ListSkeleton, errorMessage } from './CatalogStates';

/** Static marketing taglines for known brands (counts come from the live catalog). */
const BRAND_ORIGINS: Record<string, string> = {
  'Zhi-mart Fireworks': 'Direct Sivakasi Factory Direct',
  'Sivakasi Master Crafts': 'Artisanal Pyrotechnic Guild',
  'Eco-Green Pyro Labs': 'Zero Barium Certified Formulation',
};

interface BrandsModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectBrand: (brand: string) => void;
}

export const BrandsModal: React.FC<BrandsModalProps> = ({
  isOpen,
  onClose,
  onSelectBrand,
}) => {
  const productsQuery = useProducts();

  if (!isOpen) return null;

  const brands = countBy(productsQuery.data ?? [], 'brand').map((b) => ({
    name: b.name,
    items: b.count,
    origin: BRAND_ORIGINS[b.name] ?? 'Sivakasi fireworks',
  }));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 text-left"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between pb-4 border-b border-neutral-100">
          <div className="flex items-center gap-2">
            <Tag className="w-5 h-5 text-amber-600" />
            <h3 className="text-lg font-bold text-slate-900">
              Brands By Product
            </h3>
          </div>
          <button onClick={onClose} className="p-1.5 rounded-full hover:bg-neutral-100">
            <X className="w-5 h-5 text-slate-500" />
          </button>
        </div>

        <div className="py-4 space-y-3 max-h-[60vh] overflow-y-auto">
          {productsQuery.isPending && <ListSkeleton rows={3} />}
          {productsQuery.isError && !productsQuery.data && (
            <CatalogErrorCard
              message={errorMessage(productsQuery.error)}
              onRetry={() => void productsQuery.refetch()}
              isRetrying={productsQuery.isFetching}
            />
          )}
          {productsQuery.data && brands.length === 0 && (
            <p className="py-8 text-center text-sm font-semibold text-slate-500">No brands available yet.</p>
          )}
          {brands.map((b) => (
            <div
              key={b.name}
              onClick={() => {
                onSelectBrand(b.name);
                onClose();
              }}
              className="p-4 rounded-2xl border border-neutral-200 hover:border-slate-800 hover:bg-slate-50 cursor-pointer transition-all flex items-center justify-between group"
            >
              <div>
                <h4 className="text-sm font-bold text-slate-900 group-hover:text-amber-600">
                  {b.name}
                </h4>
                <p className="text-xs text-slate-500">{b.origin}</p>
              </div>
              <span className="text-xs bg-amber-50 text-amber-800 font-bold px-3 py-1 rounded-full">
                {b.items} {b.items === 1 ? 'Product' : 'Products'}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
