import React, { useState, useEffect } from 'react';
import {
  X,
  Mail,
  Lock,
  Eye,
  EyeOff,
  ShieldCheck,
  AlertCircle,
  ArrowRight,
} from 'lucide-react';
import { UserProfile } from '../types';

interface AuthModalProps {
  isOpen: boolean;
  initialStep?: 'login';
  onClose: () => void;
  onLoginSuccess: (user: UserProfile) => void;
  onNavigateToAdmin?: () => void;
}

export const AuthModal: React.FC<AuthModalProps> = ({
  isOpen,
  onClose,
  onLoginSuccess,
  onNavigateToAdmin,
}) => {
  const [loginEmail, setLoginEmail] = useState('admin@zhimart.com');
  const [loginPassword, setLoginPassword] = useState('admin123');
  const [showPassword, setShowPassword] = useState(false);
  const [errorMsg, setErrorMsg] = useState('');

  useEffect(() => {
    if (isOpen) {
      setErrorMsg('');
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleAdminLogin = (e: React.FormEvent) => {
    e.preventDefault();
    if (loginEmail === 'admin@zhimart.com' && loginPassword === 'admin123') {
      const adminUser: UserProfile = {
        name: 'Admin (Operations)',
        phone: '+91 98400 12345',
        email: loginEmail,
        isLoggedIn: true,
        isAdmin: true,
      };
      localStorage.setItem('zhi_mart_admin', 'true');
      onLoginSuccess(adminUser);
      onClose();
      if (onNavigateToAdmin) {
        onNavigateToAdmin();
      }
    } else {
      setErrorMsg('Invalid admin credentials. Use admin@zhimart.com / admin123');
    }
  };

  const handleQuickDemo = () => {
    setLoginEmail('admin@zhimart.com');
    setLoginPassword('admin123');
    setErrorMsg('');
    const adminUser: UserProfile = {
      name: 'Admin (Operations)',
      phone: '+91 98400 12345',
      email: 'admin@zhimart.com',
      isLoggedIn: true,
      isAdmin: true,
    };
    localStorage.setItem('zhi_mart_admin', 'true');
    onLoginSuccess(adminUser);
    onClose();
    if (onNavigateToAdmin) {
      onNavigateToAdmin();
    }
  };

  return (
    <div className="fixed inset-0 z-50 bg-slate-950/70 backdrop-blur-xs flex items-center justify-center p-4 animate-in fade-in duration-200">
      <div className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-slate-200 p-7 sm:p-8 relative text-left animate-in zoom-in-95 duration-150">
        {/* Close Button */}
        <button
          onClick={onClose}
          className="absolute top-5 right-5 p-2 rounded-xl text-slate-400 hover:text-slate-700 hover:bg-slate-100 transition-colors"
          aria-label="Close"
        >
          <X className="w-5 h-5" />
        </button>

        {/* Top Header */}
        <div className="flex flex-col items-center text-center mb-6">
          <div className="w-12 h-12 rounded-2xl bg-gradient-to-tr from-slate-900 to-slate-800 p-0.5 shadow-xs mb-3 flex items-center justify-center">
            <span
              className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-xl leading-none"
              style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
            >
              ழி
            </span>
          </div>
          <h2 className="text-xl font-black text-slate-950 tracking-tight font-heading">
            Admin Portal Login
          </h2>
          <p className="text-[11px] font-bold text-amber-600 uppercase tracking-wider mt-0.5">
            Zhi-mart Sivakasi Operations
          </p>
          <p className="text-xs text-slate-500 mt-1 max-w-xs">
            Restricted staff access for order tracking, product CRUD operations, and payments.
          </p>
        </div>

        {errorMsg && (
          <div className="mb-5 p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-700 text-xs font-bold flex items-center gap-2">
            <AlertCircle className="w-4 h-4 shrink-0" />
            <span>{errorMsg}</span>
          </div>
        )}

        <form onSubmit={handleAdminLogin} className="space-y-4">
          <div>
            <label className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5">
              Admin Email ID
            </label>
            <div className="relative">
              <Mail className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
              <input
                type="email"
                value={loginEmail}
                onChange={(e) => setLoginEmail(e.target.value)}
                placeholder="admin@zhimart.com"
                required
                className="w-full pl-10 pr-4 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
              />
            </div>
          </div>

          <div>
            <label className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5">
              Admin Password
            </label>
            <div className="relative">
              <Lock className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
              <input
                type={showPassword ? 'text' : 'password'}
                value={loginPassword}
                onChange={(e) => setLoginPassword(e.target.value)}
                placeholder="••••••••"
                required
                className="w-full pl-10 pr-10 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="absolute right-3.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600"
              >
                {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
          </div>

          <button
            type="submit"
            className="w-full py-3 px-4 bg-slate-950 hover:bg-slate-900 text-white font-black text-xs rounded-xl transition-all shadow-md active:scale-98 flex items-center justify-center gap-2"
          >
            <ShieldCheck className="w-4 h-4 text-amber-400" />
            <span>Sign In to Admin Console</span>
          </button>
        </form>

        <div className="mt-4 pt-4 border-t border-slate-100 flex flex-col gap-2">
          <button
            type="button"
            onClick={handleQuickDemo}
            className="w-full py-2.5 px-3 bg-amber-50 hover:bg-amber-100/80 border border-amber-200 text-amber-900 text-xs font-black rounded-xl transition-all flex items-center justify-center gap-1.5"
          >
            <span>⚡ One-Click Demo Admin Login</span>
          </button>

          {onNavigateToAdmin && (
            <button
              type="button"
              onClick={() => {
                onClose();
                onNavigateToAdmin();
              }}
              className="w-full py-2 text-xs font-bold text-slate-500 hover:text-slate-900 flex items-center justify-center gap-1"
            >
              <span>Go to Fullscreen Admin Page</span>
              <ArrowRight className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
