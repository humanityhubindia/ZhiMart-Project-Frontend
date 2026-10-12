import React from 'react';
import { X, Tag } from 'lucide-react';

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
  if (!isOpen) return null;

  const brands = [
    { name: 'Zhi-mart Fireworks', origin: 'Direct Sivakasi Factory Direct', items: 15 },
    { name: 'Sivakasi Master Crafts', origin: 'Artisanal Pyrotechnic Guild', items: 10 },
    { name: 'Eco-Green Pyro Labs', origin: 'Zero Barium Certified Formulation', items: 8 },
  ];

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

        <div className="py-4 space-y-3">
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
                {b.items} Products
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
