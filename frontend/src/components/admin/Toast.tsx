import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, BellRing, CheckCircle2, Info, X } from 'lucide-react';

type ToastTone = 'success' | 'error' | 'info' | 'order';

interface ToastItem {
  id: number;
  tone: ToastTone;
  message: string;
  action?: { label: string; onClick: () => void };
}

interface ToastApi {
  success: (message: string) => void;
  error: (message: string) => void;
  info: (message: string) => void;
  order: (message: string, action?: ToastItem['action']) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

const DURATION: Record<ToastTone, number> = { success: 3500, info: 4000, error: 7000, order: 8000 };
const MAX_TOASTS = 4;

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    setToasts((list) => list.filter((t) => t.id !== id));
    const timer = timers.current.get(id);
    if (timer) clearTimeout(timer);
    timers.current.delete(id);
  }, []);

  const push = useCallback(
    (tone: ToastTone, message: string, action?: ToastItem['action']) => {
      const id = nextId.current++;
      setToasts((list) => [...list.slice(-(MAX_TOASTS - 1)), { id, tone, message, action }]);
      timers.current.set(
        id,
        setTimeout(() => dismiss(id), DURATION[tone])
      );
    },
    [dismiss]
  );

  useEffect(() => {
    const map = timers.current;
    return () => map.forEach((t) => clearTimeout(t));
  }, []);

  const api = useMemo<ToastApi>(
    () => ({
      success: (m) => push('success', m),
      error: (m) => push('error', m),
      info: (m) => push('info', m),
      order: (m, action) => push('order', m, action),
    }),
    [push]
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div
        aria-live="polite"
        aria-atomic="false"
        className="fixed bottom-4 right-4 left-4 sm:left-auto z-[80] flex flex-col gap-2 items-stretch sm:items-end pointer-events-none"
      >
        {toasts.map((t) => {
          const Icon =
            t.tone === 'success' ? CheckCircle2 : t.tone === 'error' ? AlertTriangle : t.tone === 'order' ? BellRing : Info;
          const iconClass =
            t.tone === 'success'
              ? 'text-emerald-400'
              : t.tone === 'error'
                ? 'text-rose-400'
                : t.tone === 'order'
                  ? 'text-amber-400'
                  : 'text-sky-400';
          return (
            <div
              key={t.id}
              role={t.tone === 'error' ? 'alert' : 'status'}
              className="pointer-events-auto sm:max-w-sm bg-slate-950 text-white pl-4 pr-2 py-3 rounded-2xl shadow-xl flex items-start gap-2.5 text-xs font-bold border border-slate-800 animate-in fade-in slide-in-from-bottom-2 duration-150"
            >
              <Icon className={`w-4 h-4 shrink-0 mt-px ${iconClass}`} />
              <span className="flex-1 leading-relaxed">{t.message}</span>
              {t.action && (
                <button
                  type="button"
                  onClick={() => {
                    t.action?.onClick();
                    dismiss(t.id);
                  }}
                  className="text-amber-300 hover:text-amber-200 underline underline-offset-2 shrink-0"
                >
                  {t.action.label}
                </button>
              )}
              <button
                type="button"
                onClick={() => dismiss(t.id)}
                aria-label="Dismiss notification"
                className="p-0.5 rounded text-slate-400 hover:text-white shrink-0"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used within <ToastProvider>');
  return ctx;
}
