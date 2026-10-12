import React from 'react';
import { X, Folder } from 'lucide-react';

interface CategoriesModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectCategory: (category: string) => void;
}

export const CategoriesModal: React.FC<CategoriesModalProps> = ({
  isOpen,
  onClose,
  onSelectCategory,
}) => {
  if (!isOpen) return null;

  const categories = [
    { name: 'SINGLE SOUND CRACKERS', count: 4, desc: 'Bird crackers, Lakshmi bombs, and single loud acoustic blasts.' },
    { name: 'ELECTRIC TAPE FUSE CRACKERS', count: 2, desc: 'Fast uninterrupted 28, 56 garland strips on adhesive tape.' },
    { name: 'DELUXE WIRE FUSE CRACKERS', count: 3, desc: 'Kurukshetra 24, 50, and 100 deluxe wire fuse rolls.' },
    { name: 'FLOWER POTS', count: 3, desc: 'Ganga Jamuna, Big, and Super Deluxe sparkling cones.' },
    { name: 'GROUND CHAKKARS', count: 1, desc: 'Special ground wheels with emerald and golden rotation.' },
    { name: 'AERIAL REPEATERS', count: 1, desc: '12 multi-color vertical sky bursting shells.' },
    { name: 'GIFT BOXES & HAMPERS', count: 1, desc: 'Mega assortment wooden keepsake boxes for complete family events.' },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-lg bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 text-left"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between pb-4 border-b border-neutral-100">
          <div className="flex items-center gap-2">
            <Folder className="w-5 h-5 text-slate-700" />
            <h3 className="text-lg font-bold text-slate-900">
              Browse Categories
            </h3>
          </div>
          <button onClick={onClose} className="p-1.5 rounded-full hover:bg-neutral-100">
            <X className="w-5 h-5 text-slate-500" />
          </button>
        </div>

        <div className="py-4 space-y-2.5 max-h-[60vh] overflow-y-auto">
          {categories.map((c) => (
            <div
              key={c.name}
              onClick={() => {
                onSelectCategory(c.name);
                onClose();
              }}
              className="p-3.5 rounded-2xl border border-neutral-200 hover:border-slate-800 hover:bg-slate-50 cursor-pointer transition-all flex items-center justify-between group"
            >
              <div>
                <h4 className="text-xs font-bold text-slate-900 group-hover:text-amber-600 transition-colors">
                  {c.name}
                </h4>
                <p className="text-[11px] text-slate-500 mt-0.5">{c.desc}</p>
              </div>
              <span className="text-xs bg-slate-100 text-slate-700 font-bold px-2.5 py-1 rounded-full">
                {c.count} items
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
