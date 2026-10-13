import React from 'react';
import { X, ArrowRight, Clock, Loader2, ShieldCheck } from 'lucide-react';
import { useAdminAuth } from '../auth/AdminAuthContext';
import { AdminLoginForm } from './AdminLoginForm';

interface AuthModalProps {
  isOpen: boolean;
  initialStep?: 'login';
  onClose: () => void;
  /** Optional hook fired after a successful password + OTP sign-in. */
  onLoginSuccess?: () => void;
  onNavigateToAdmin?: () => void;
}

export const AuthModal: React.FC<AuthModalProps> = ({
  isOpen,
  onClose,
  onLoginSuccess,
  onNavigateToAdmin,
}) => {
  const { status, admin, sessionEndedReason } = useAdminAuth();

  if (!isOpen) return null;

  const handleSuccess = () => {
    onLoginSuccess?.();
    onClose();
    onNavigateToAdmin?.();
  };

  return (
    <div
      className="fixed inset-0 z-50 bg-slate-950/70 backdrop-blur-xs flex items-center justify-center p-4 animate-in fade-in duration-200"
      role="dialog"
      aria-modal="true"
      aria-labelledby="zm-auth-modal-title"
    >
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
          <h2
            id="zm-auth-modal-title"
            className="text-xl font-black text-slate-950 tracking-tight font-heading"
          >
            Admin Portal Login
          </h2>
          <p className="text-[11px] font-bold text-amber-600 uppercase tracking-wider mt-0.5">
            Zhi-mart Sivakasi Operations
          </p>
          <p className="text-xs text-slate-500 mt-1 max-w-xs">
            Restricted staff access for order tracking, product CRUD operations, and payments.
          </p>
        </div>

        {status === 'loading' ? (
          <div role="status" className="py-8 flex flex-col items-center gap-2 text-slate-500">
            <Loader2 className="w-6 h-6 text-amber-500 animate-spin" />
            <span className="text-xs font-bold uppercase tracking-wider">Verifying session…</span>
          </div>
        ) : status === 'authenticated' ? (
          <div className="space-y-4">
            <p className="text-xs text-slate-600 text-center">
              Signed in as{' '}
              <span className="font-extrabold text-slate-900">{admin?.email}</span>
            </p>
            <button
              type="button"
              onClick={handleSuccess}
              className="w-full py-3 px-4 bg-slate-950 hover:bg-slate-900 text-white font-black text-xs rounded-xl transition-all shadow-md active:scale-98 flex items-center justify-center gap-2"
            >
              <ShieldCheck className="w-4 h-4 text-amber-400" />
              <span>Open Admin Console</span>
            </button>
          </div>
        ) : (
          <>
            {sessionEndedReason && (
              <div
                role="status"
                className="mb-5 p-3 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs font-bold flex items-center gap-2"
              >
                <Clock className="w-4 h-4 shrink-0 text-amber-600" />
                <span>{sessionEndedReason}</span>
              </div>
            )}
            <AdminLoginForm compact onSuccess={handleSuccess} />
          </>
        )}

        {onNavigateToAdmin && status !== 'authenticated' && (
          <div className="mt-4 pt-4 border-t border-slate-100 flex flex-col gap-2">
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
          </div>
        )}
      </div>
    </div>
  );
};
