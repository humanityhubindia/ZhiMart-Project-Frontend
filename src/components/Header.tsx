import React, { useState } from 'react';
import { ShoppingCart, ChevronDown, Flame } from 'lucide-react';
import { UserProfile } from '../types';

interface HeaderProps {
  onOpenCart: () => void;
  onOpenAuth: () => void;
  onOpenQuickEstimate: () => void;
  onOpenAIAssistant: () => void;
  cartCount: number;
  cartTotal: number;
  user: UserProfile;
  onSelectCategory: (category: string) => void;
}

export const Header: React.FC<HeaderProps> = ({
  onOpenCart,
  onOpenAuth,
  onOpenQuickEstimate,
  onOpenAIAssistant,
  cartCount,
  cartTotal,
  user,
  onSelectCategory,
}) => {
  const [productsDropdownOpen, setProductsDropdownOpen] = useState(false);
  const [supportDropdownOpen, setSupportDropdownOpen] = useState(false);

  const categories = [
    'SINGLE SOUND CRACKERS',
    'ELECTRIC TAPE FUSE CRACKERS',
    'DELUXE WIRE FUSE CRACKERS',
    'FLOWER POTS',
    'GROUND CHAKKARS',
    'AERIAL REPEATERS',
    'GIFT BOXES & HAMPERS',
  ];

  return (
    <header className="w-full bg-[#0F172A] text-white border-b border-slate-800 sticky top-0 z-40 shadow-sm">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 h-16 flex items-center justify-between">
        {/* Left: Brand Logo & Title */}
        <div className="flex items-center space-x-3 cursor-pointer" onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })}>
          <div className="w-9 h-9 rounded-xl bg-gradient-to-tr from-amber-500 to-red-500 flex items-center justify-center shadow-md">
            <Flame className="w-5 h-5 text-white fill-white" />
          </div>
          <span className="text-xl font-bold tracking-tight text-white font-heading">
            Zhi-mart Crackers
          </span>
        </div>

        {/* Center: Navigation Links */}
        <nav className="hidden lg:flex items-center space-x-7 text-sm font-medium text-slate-300">
          <a
            href="#"
            className="text-white hover:text-amber-400 transition-colors"
          >
            Home
          </a>

          <button
            onClick={onOpenQuickEstimate}
            className="hover:text-amber-400 transition-colors flex items-center gap-1"
          >
            <span>Quick Estimate</span>
            <span className="text-[10px] bg-red-600 text-white px-1.5 py-0.2 rounded font-bold">HOT</span>
          </button>

          <button
            onClick={onOpenAIAssistant}
            className="hover:text-amber-400 transition-colors flex items-center gap-1"
          >
            <span>AI Assistant</span>
          </button>

          {/* Products Dropdown */}
          <div className="relative">
            <button
              onClick={() => setProductsDropdownOpen(!productsDropdownOpen)}
              onBlur={() => setTimeout(() => setProductsDropdownOpen(false), 200)}
              className="flex items-center gap-1 hover:text-amber-400 transition-colors"
            >
              <span>Products</span>
              <ChevronDown className="w-4 h-4 text-slate-400" />
            </button>

            {productsDropdownOpen && (
              <div className="absolute left-0 mt-3 w-64 bg-slate-900 border border-slate-800 rounded-2xl shadow-2xl py-2 z-50 text-xs">
                {categories.map((cat) => (
                  <button
                    key={cat}
                    onClick={() => {
                      onSelectCategory(cat);
                      setProductsDropdownOpen(false);
                      const el = document.getElementById('products-section');
                      if (el) el.scrollIntoView({ behavior: 'smooth' });
                    }}
                    className="w-full text-left px-4 py-2.5 text-slate-300 hover:text-white hover:bg-slate-800/80 transition-colors font-medium"
                  >
                    {cat}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Support Dropdown */}
          <div className="relative">
            <button
              onClick={() => setSupportDropdownOpen(!supportDropdownOpen)}
              onBlur={() => setTimeout(() => setSupportDropdownOpen(false), 200)}
              className="flex items-center gap-1 hover:text-amber-400 transition-colors"
            >
              <span>Support</span>
              <ChevronDown className="w-4 h-4 text-slate-400" />
            </button>

            {supportDropdownOpen && (
              <div className="absolute left-0 mt-3 w-52 bg-slate-900 border border-slate-800 rounded-2xl shadow-2xl py-2 z-50 text-xs">
                <a
                  href="#contact"
                  className="block px-4 py-2.5 text-slate-300 hover:text-white hover:bg-slate-800/80"
                >
                  24/7 Helpline: +91 98400 12345
                </a>
                <a
                  href="#safety"
                  className="block px-4 py-2.5 text-slate-300 hover:text-white hover:bg-slate-800/80"
                >
                  Diwali Safety Guidelines
                </a>
                <a
                  href="#license"
                  className="block px-4 py-2.5 text-slate-300 hover:text-white hover:bg-slate-800/80"
                >
                  PESO Govt License & Certs
                </a>
              </div>
            )}
          </div>

          <a
            href="#lifestyle"
            className="flex items-center gap-1 hover:text-amber-400 transition-colors"
          >
            <span>Lifestyle</span>
            <ChevronDown className="w-4 h-4 text-slate-400" />
          </a>
        </nav>

        {/* Right: Cart Pill & Login Button */}
        <div className="flex items-center space-x-3">
          {/* Cart Capsule matching Screenshot 2 */}
          <button
            onClick={onOpenCart}
            className="bg-[#1E293B] hover:bg-[#283548] border border-slate-700 hover:border-slate-600 px-3.5 py-1.5 rounded-full flex items-center space-x-2 transition-all active:scale-95"
          >
            <div className="relative flex items-center">
              <ShoppingCart className="w-4 h-4 text-white" />
              {cartCount > 0 && (
                <span className="absolute -top-2 -right-2 bg-red-500 text-white text-[10px] font-black w-4 h-4 rounded-full flex items-center justify-center">
                  {cartCount}
                </span>
              )}
            </div>
            <span className="text-sm font-bold text-white tracking-wide">
              ₹{cartTotal}
            </span>
          </button>

          {/* Login Button matching Screenshot 2 */}
          <button
            onClick={onOpenAuth}
            className="bg-[#F59E0B] hover:bg-[#D97706] active:scale-95 text-[#0F172A] font-bold text-xs sm:text-sm px-4 sm:px-5 py-2 rounded-xl transition-all shadow-xs"
          >
            {user.isLoggedIn ? (user.name.split(' ')[0] || 'Account') : 'Login'}
          </button>
        </div>
      </div>
    </header>
  );
};
