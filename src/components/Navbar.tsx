import React, { useState, useRef, useEffect } from 'react';
import {
  Menu,
  User,
  ShoppingCart,
  MessageCircle,
  FileSpreadsheet,
  LogIn,
  LogOut,
  Flame,
  Package,
  ShieldCheck,
} from 'lucide-react';
import { UserProfile } from '../types';

interface NavbarProps {
  onOpenAuth: (step?: 'login') => void;
  onOpenCart: () => void;
  onOpenQuickEstimate: () => void;
  onOpenAIAssistant: () => void;
  onNavigateAdmin: () => void;
  cartCount: number;
  cartTotal?: number;
  user: UserProfile;
  onLogout: () => void;
  onNavigateHome: () => void;
  onNavigateCatalog?: () => void;
  currentRoute?: string;
}

export const Navbar: React.FC<NavbarProps> = ({
  onOpenAuth,
  onOpenCart,
  onOpenQuickEstimate,
  onOpenAIAssistant,
  onNavigateAdmin,
  cartCount,
  cartTotal = 0,
  user,
  onLogout,
  onNavigateHome,
  currentRoute = 'home',
}) => {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSupportClick = () => {
    window.open(
      'https://wa.me/919840012345?text=Hello%20Zhi-mart%20Support,%20I%20need%20help%20with%20crackers%20order',
      '_blank'
    );
  };

  return (
    <header className="w-full bg-white/95 backdrop-blur-md border-b border-slate-200/80 sticky top-0 z-40 shadow-[0_2px_15px_-4px_rgba(0,0,0,0.04)]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-20 flex items-center justify-between gap-4">
        {/* Left: Premium Brand Identity */}
        <div className="flex items-center space-x-3">
          <button
            onClick={onNavigateHome}
            className="flex items-center gap-3 group text-left transition-transform active:scale-98 select-none"
            aria-label="Zhi-mart Home"
          >
            {/* Geometric Brand Emblem with Tamil 'ழி' */}
            <div className="w-10 h-10 rounded-2xl bg-gradient-to-tr from-slate-900 via-slate-800 to-amber-500 p-0.5 shadow-xs group-hover:shadow-sm transition-all shrink-0">
              <div className="w-full h-full bg-slate-950 rounded-[14px] flex items-center justify-center">
                <span
                  className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-base sm:text-lg select-none leading-none pt-0.5"
                  style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                >
                  ழி
                </span>
              </div>
            </div>

            {/* Brand Title & Tagline */}
            <div className="flex flex-col">
              <span className="text-xl sm:text-2xl font-black tracking-tight text-slate-900 font-heading leading-tight group-hover:text-amber-600 transition-colors">
                ZHI-MART
              </span>
              <span className="text-[9px] font-extrabold uppercase tracking-widest text-slate-400 -mt-0.5">
                Sivakasi Direct
              </span>
            </div>
          </button>
        </div>

        {/* Center: Segmented Navigation Track */}
        <nav className="hidden md:flex items-center p-1.5 bg-slate-100/80 rounded-2xl border border-slate-200/70 shadow-2xs">
          {/* Home */}
          <button
            onClick={onNavigateHome}
            className={`px-4 py-2 rounded-xl text-xs transition-all ${
              currentRoute === 'home'
                ? 'bg-white text-slate-950 font-extrabold shadow-2xs'
                : 'text-slate-600 hover:text-slate-950 font-bold hover:bg-white/60'
            }`}
          >
            Home
          </button>

          {/* Quick Estimate */}
          <button
            onClick={onOpenQuickEstimate}
            className={`px-4 py-2 rounded-xl text-xs transition-all flex items-center gap-1.5 ${
              currentRoute === 'estimate'
                ? 'bg-white text-slate-950 font-extrabold shadow-2xs'
                : 'text-slate-600 hover:text-slate-950 font-bold hover:bg-white/60'
            }`}
          >
            <span>Quick Estimate</span>
            <span className="text-[9px] bg-red-600 text-white px-1.5 py-0.2 rounded-md font-black tracking-wide shadow-2xs uppercase">
              HOT
            </span>
          </button>

          {/* AI Assistant */}
          <button
            onClick={onOpenAIAssistant}
            className={`px-4 py-2 rounded-xl text-xs transition-all ${
              currentRoute === 'ai-assistant'
                ? 'bg-white text-slate-950 font-extrabold shadow-2xs'
                : 'text-slate-600 hover:text-slate-950 font-bold hover:bg-white/60'
            }`}
          >
            AI Assistant
          </button>

          {/* Support */}
          <button
            onClick={handleSupportClick}
            className="px-4 py-2 rounded-xl text-xs font-bold text-slate-600 hover:text-slate-950 hover:bg-white/60 transition-all flex items-center gap-1.5"
          >
            <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
            <span>Support</span>
          </button>
        </nav>

        {/* Right Actions: Cart Pill + User Profile */}
        <div className="flex items-center space-x-3">
          {/* Cart Pill */}
          <button
            onClick={onOpenCart}
            className={`group relative flex items-center gap-2.5 px-4 py-2.5 rounded-2xl transition-all shadow-xs hover:shadow-md active:scale-97 border ${
              currentRoute === 'cart'
                ? 'bg-slate-950 text-white border-amber-400 ring-2 ring-amber-400/20'
                : 'bg-slate-900 hover:bg-slate-950 text-white border-slate-800'
            }`}
            title="Shopping Cart"
          >
            <div className="relative flex items-center">
              <ShoppingCart className="w-4 h-4 text-amber-400 group-hover:scale-105 transition-transform" />
              {cartCount > 0 && (
                <span className="absolute -top-2.5 -right-2.5 bg-red-500 text-white text-[10px] font-black w-4 h-4 rounded-full flex items-center justify-center shadow-xs">
                  {cartCount}
                </span>
              )}
            </div>
            <div className="h-3.5 w-[1px] bg-slate-700" />
            <span className="text-xs font-extrabold text-white tracking-wide">
              ₹{cartTotal}
            </span>
          </button>

          {/* User Profile Capsule with Dropdown */}
          <div className="relative" ref={menuRef}>
            <button
              onClick={() => setMenuOpen(!menuOpen)}
              className="flex items-center gap-2.5 bg-white hover:bg-slate-50 border border-slate-200 hover:border-slate-300 py-1.5 px-3 rounded-2xl transition-all shadow-2xs active:scale-97 text-slate-700"
              aria-label="User Account Menu"
            >
              <Menu className="w-4 h-4 text-slate-500" />
              <div className="w-7 h-7 rounded-xl bg-slate-100 border border-slate-200 text-slate-700 flex items-center justify-center overflow-hidden font-bold text-xs">
                {user.isLoggedIn ? (
                  user.name.charAt(0).toUpperCase()
                ) : (
                  <User className="w-3.5 h-3.5 text-slate-500" />
                )}
              </div>
            </button>

            {/* Dropdown Menu */}
            {menuOpen && (
              <div className="absolute right-0 mt-2 w-60 bg-white rounded-2xl shadow-xl border border-slate-200 p-2 z-50 text-left animate-in fade-in zoom-in-95 duration-100">
                {user.isLoggedIn ? (
                  <>
                    <div className="px-4 py-2.5 border-b border-slate-100">
                      <p className="text-[10px] text-slate-400 font-bold uppercase tracking-wider">
                        Signed in as
                      </p>
                      <p className="text-xs font-black text-slate-900 truncate">
                        {user.name}
                      </p>
                    </div>

                    <div className="md:hidden py-1 border-b border-slate-100">
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onNavigateHome();
                        }}
                        className="w-full text-left px-3.5 py-2 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl transition-colors"
                      >
                        Home
                      </button>
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenAIAssistant();
                        }}
                        className="w-full text-left px-3.5 py-2 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl transition-colors"
                      >
                        AI Assistant
                      </button>
                    </div>

                    <div className="py-1">
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onNavigateAdmin();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-black text-amber-950 bg-amber-50 hover:bg-amber-100/90 rounded-xl flex items-center justify-between transition-colors mb-1 border border-amber-200/60"
                      >
                        <div className="flex items-center gap-2">
                          <ShieldCheck className="w-3.5 h-3.5 text-amber-600" />
                          <span>Admin Control Console</span>
                        </div>
                        <span className="text-[9px] bg-amber-200 text-amber-900 font-extrabold px-1.5 py-0.5 rounded uppercase">
                          Portal
                        </span>
                      </button>

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenCart();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center justify-between transition-colors"
                      >
                        <div className="flex items-center gap-2">
                          <Package className="w-3.5 h-3.5 text-slate-400" />
                          <span>Orders & Bag</span>
                        </div>
                        <span className="text-[10px] bg-slate-100 px-2 py-0.5 rounded-full font-extrabold text-slate-800">
                          {cartCount} (₹{cartTotal})
                        </span>
                      </button>

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenQuickEstimate();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center gap-2 transition-colors"
                      >
                        <FileSpreadsheet className="w-3.5 h-3.5 text-slate-400" />
                        <span>Quick Estimate Sheet</span>
                      </button>

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          handleSupportClick();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center gap-2 transition-colors"
                      >
                        <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
                        <span>Support & WhatsApp</span>
                      </button>
                    </div>

                    <div className="border-t border-slate-100 pt-1">
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onLogout();
                        }}
                        className="w-full text-left px-3.5 py-2 text-xs font-bold text-rose-600 hover:bg-rose-50 rounded-xl flex items-center gap-2 transition-colors"
                      >
                        <LogOut className="w-3.5 h-3.5" />
                        <span>Log Out</span>
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="md:hidden py-1 border-b border-slate-100">
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onNavigateHome();
                        }}
                        className="w-full text-left px-3.5 py-2 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl transition-colors"
                      >
                        Home
                      </button>
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenAIAssistant();
                        }}
                        className="w-full text-left px-3.5 py-2 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl transition-colors"
                      >
                        AI Assistant
                      </button>
                    </div>

                    <div className="py-1">
                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onNavigateAdmin();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-black text-slate-950 bg-amber-50 hover:bg-amber-100/90 border border-amber-200/80 rounded-xl flex items-center justify-between transition-colors mb-1.5 shadow-2xs"
                      >
                        <div className="flex items-center gap-2">
                          <ShieldCheck className="w-3.5 h-3.5 text-amber-600" />
                          <span>Admin Login</span>
                        </div>
                        <span className="text-[9px] font-black uppercase text-amber-900 bg-amber-200/70 px-1.5 py-0.5 rounded">
                          Console
                        </span>
                      </button>

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenCart();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center justify-between transition-colors"
                      >
                        <div className="flex items-center gap-2">
                          <Package className="w-3.5 h-3.5 text-slate-400" />
                          <span>View Cart</span>
                        </div>
                        <span className="text-[10px] bg-amber-100 text-amber-900 font-extrabold px-2 py-0.5 rounded-full">
                          {cartCount} (₹{cartTotal})
                        </span>
                      </button>

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          onOpenQuickEstimate();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center gap-2 transition-colors"
                      >
                        <FileSpreadsheet className="w-3.5 h-3.5 text-slate-400" />
                        <span>Quick Estimate Sheet</span>
                      </button>

                      <div className="my-1 border-t border-slate-100" />

                      <button
                        onClick={() => {
                          setMenuOpen(false);
                          handleSupportClick();
                        }}
                        className="w-full text-left px-3.5 py-2.5 text-xs font-bold text-slate-700 hover:bg-slate-50 rounded-xl flex items-center gap-2 transition-colors"
                      >
                        <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
                        <span>Support & WhatsApp</span>
                      </button>
                    </div>
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </header>
  );
};
