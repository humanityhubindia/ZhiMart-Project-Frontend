import React, { useEffect, useId, useRef, useState } from 'react';
import { AlertTriangle, FlaskConical, ImageOff, Inbox, Loader2, RefreshCw, X } from 'lucide-react';
import type { InventoryStatus, OrderStatus, PaymentStatus, PricingTier } from '../../types/admin';
import { usePricingRules } from '../../hooks/useProducts';
import {
  errorMessage,
  INVENTORY_STATUS_META,
  ORDER_STATUS_META,
  PAYMENT_STATUS_META,
} from './format';

// ---------------------------------------------------------------------------
// Class helpers
// ---------------------------------------------------------------------------

export const inputClass =
  'w-full px-3.5 py-2.5 bg-white border border-slate-200 rounded-xl text-xs font-semibold text-slate-900 placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-amber-500 disabled:bg-slate-50 disabled:text-slate-500';
export const inputErrorClass = 'border-rose-400 focus:ring-rose-500';
export const labelClass = 'block text-[11px] font-extrabold uppercase tracking-wider text-slate-500 mb-1.5';

const buttonBase =
  'inline-flex items-center justify-center gap-1.5 rounded-xl text-xs font-bold transition-all disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:ring-offset-1';

const buttonVariants = {
  primary: 'bg-slate-950 hover:bg-slate-800 text-white shadow-xs',
  accent: 'bg-amber-500 hover:bg-amber-400 text-slate-950 font-black shadow-xs',
  secondary: 'bg-white hover:bg-slate-50 text-slate-800 border border-slate-200',
  ghost: 'text-slate-600 hover:text-slate-900 hover:bg-slate-100',
  danger: 'bg-rose-600 hover:bg-rose-500 text-white shadow-xs',
  dangerOutline: 'text-rose-700 hover:bg-rose-50 border border-rose-200 bg-white',
  success: 'bg-emerald-600 hover:bg-emerald-500 text-white shadow-xs',
} as const;

const buttonSizes = {
  sm: 'px-2.5 py-1.5',
  md: 'px-3.5 py-2',
  lg: 'px-5 py-2.5',
} as const;

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof buttonVariants;
  size?: keyof typeof buttonSizes;
  loading?: boolean;
  icon?: React.ComponentType<{ className?: string }>;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = 'secondary', size = 'md', loading, icon: Icon, className = '', disabled, children, type, ...rest },
  ref
) {
  return (
    <button
      ref={ref}
      type={type ?? 'button'}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={`${buttonBase} ${buttonVariants[variant]} ${buttonSizes[size]} ${className}`}
      {...rest}
    >
      {loading ? (
        <Loader2 className="w-3.5 h-3.5 animate-spin" aria-hidden />
      ) : Icon ? (
        <Icon className="w-3.5 h-3.5" />
      ) : null}
      {children}
    </button>
  );
});

// ---------------------------------------------------------------------------
// Status primitives
// ---------------------------------------------------------------------------

export function Spinner({ className = 'w-4 h-4', label }: { className?: string; label?: string }) {
  return (
    <span role="status" className="inline-flex items-center gap-2">
      <Loader2 className={`${className} animate-spin text-amber-500`} aria-hidden />
      <span className={label ? 'text-xs font-bold text-slate-500' : 'sr-only'}>{label ?? 'Loading'}</span>
    </span>
  );
}

export function Skeleton({ className = '' }: { className?: string }) {
  return <div aria-hidden className={`animate-pulse rounded-xl bg-slate-200/70 ${className}`} />;
}

export function ErrorCard({
  title = 'Could not load data',
  error,
  onRetry,
  retrying,
  compact,
}: {
  title?: string;
  error: unknown;
  onRetry?: () => void;
  retrying?: boolean;
  compact?: boolean;
}) {
  return (
    <div
      role="alert"
      className={`bg-white rounded-3xl border border-rose-200 ${compact ? 'p-4' : 'p-8'} flex flex-col sm:flex-row sm:items-center gap-4`}
    >
      <div className="w-10 h-10 rounded-2xl bg-rose-50 text-rose-600 flex items-center justify-center shrink-0">
        <AlertTriangle className="w-5 h-5" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-extrabold text-slate-900">{title}</p>
        <p className="text-xs text-slate-500 mt-0.5 break-words">{errorMessage(error)}</p>
      </div>
      {onRetry && (
        <Button variant="secondary" icon={RefreshCw} onClick={onRetry} loading={retrying}>
          Retry
        </Button>
      )}
    </div>
  );
}

export function EmptyState({
  icon: Icon = Inbox,
  title,
  description,
  action,
}: {
  icon?: React.ComponentType<{ className?: string }>;
  title: string;
  description?: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="bg-white p-10 text-center rounded-3xl border border-slate-200">
      <Icon className="w-10 h-10 text-slate-300 mx-auto mb-3" />
      <p className="text-sm font-extrabold text-slate-800">{title}</p>
      {description && <p className="text-xs text-slate-500 mt-1">{description}</p>}
      {action && <div className="mt-4 flex justify-center">{action}</div>}
    </div>
  );
}

function Pill({ className, children }: { className: string; children: React.ReactNode }) {
  return (
    <span
      className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-extrabold border whitespace-nowrap ${className}`}
    >
      {children}
    </span>
  );
}

export function OrderStatusBadge({ status }: { status: OrderStatus }) {
  const meta = ORDER_STATUS_META[status] ?? ORDER_STATUS_META.pending_pack;
  return <Pill className={meta.className}>{meta.label}</Pill>;
}

export function PaymentStatusBadge({ status }: { status: PaymentStatus }) {
  const meta = PAYMENT_STATUS_META[status] ?? PAYMENT_STATUS_META.pending;
  return <Pill className={meta.className}>{meta.label}</Pill>;
}

export function InventoryStatusBadge({ status }: { status: InventoryStatus }) {
  const meta = INVENTORY_STATUS_META[status] ?? INVENTORY_STATUS_META.in_stock;
  return <Pill className={meta.className}>{meta.label}</Pill>;
}

export function TestBadge() {
  return (
    <Pill className="bg-violet-50 text-violet-800 border-violet-200">
      <FlaskConical className="w-3 h-3" aria-hidden />
      Test
    </Pill>
  );
}

/** "85% bulk" / "80% standard" — the price tier an order was charged at. */
export function PricingTierBadge({ tier }: { tier: PricingTier }) {
  const rules = usePricingRules();
  const bulk = tier === 'bulk';
  return (
    <span
      title={
        bulk
          ? `Charged at price-list (bulk) prices — order reached ₹${rules.bulkMinOrder.toLocaleString('en-IN')}`
          : `Charged at standard prices — order was below ₹${rules.bulkMinOrder.toLocaleString('en-IN')}`
      }
    >
      <Pill
        className={
          bulk ? 'bg-emerald-50 text-emerald-800 border-emerald-200' : 'bg-sky-50 text-sky-800 border-sky-200'
        }
      >
        {bulk ? `${rules.bulkDiscountPercent}% bulk` : `${rules.standardDiscountPercent}% standard`}
      </Pill>
    </span>
  );
}

export function AttentionBadge({ title }: { title?: string | null }) {
  return (
    <span title={title ?? undefined}>
      <Pill className="bg-rose-600 text-white border-rose-600">
        <AlertTriangle className="w-3 h-3" aria-hidden />
        Needs attention
      </Pill>
    </span>
  );
}

export function ArchivedBadge() {
  return <Pill className="bg-slate-100 text-slate-600 border-slate-300">Archived</Pill>;
}

// ---------------------------------------------------------------------------
// Product thumbnail with broken-image fallback
// ---------------------------------------------------------------------------

export function ProductImage({ src, alt, className = 'w-12 h-12' }: { src: string; alt: string; className?: string }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [src]);
  if (!src || broken) {
    return (
      <div
        className={`${className} rounded-xl bg-slate-100 border border-slate-200 flex items-center justify-center text-slate-400 shrink-0`}
        role="img"
        aria-label={`${alt} (image unavailable)`}
      >
        <ImageOff className="w-4 h-4" />
      </div>
    );
  }
  return (
    <img
      src={src}
      alt={alt}
      loading="lazy"
      onError={() => setBroken(true)}
      className={`${className} rounded-xl object-cover bg-slate-100 border border-slate-200 shrink-0`}
    />
  );
}

// ---------------------------------------------------------------------------
// KPI card
// ---------------------------------------------------------------------------

export function KpiCard({
  label,
  value,
  hint,
  icon: Icon,
  tone = 'slate',
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  hint?: React.ReactNode;
  icon: React.ComponentType<{ className?: string }>;
  tone?: 'slate' | 'amber' | 'emerald' | 'blue' | 'rose' | 'purple';
  onClick?: () => void;
}) {
  const toneClass = {
    slate: 'bg-slate-100 text-slate-700',
    amber: 'bg-amber-50 text-amber-700',
    emerald: 'bg-emerald-50 text-emerald-700',
    blue: 'bg-blue-50 text-blue-700',
    rose: 'bg-rose-50 text-rose-700',
    purple: 'bg-purple-50 text-purple-700',
  }[tone];
  const content = (
    <>
      <div className="flex items-center justify-between text-slate-500 mb-2 gap-2">
        <span className="text-[11px] font-bold uppercase tracking-wider text-left">{label}</span>
        <div className={`w-9 h-9 rounded-xl flex items-center justify-center shrink-0 ${toneClass}`}>
          <Icon className="w-4 h-4" />
        </div>
      </div>
      <div className="text-2xl font-black text-slate-950 tracking-tight font-heading text-left tabular-nums">
        {value}
      </div>
      {hint && <div className="mt-2 text-xs font-semibold text-slate-500 text-left">{hint}</div>}
    </>
  );
  const cls = 'bg-white p-5 rounded-3xl border border-slate-200/90 shadow-2xs w-full';
  return onClick ? (
    <button type="button" onClick={onClick} className={`${cls} hover:border-amber-300 hover:shadow-xs transition-all`}>
      {content}
    </button>
  ) : (
    <div className={cls}>{content}</div>
  );
}

// ---------------------------------------------------------------------------
// Modal / drawer with a simple stack so Escape closes only the topmost layer
// ---------------------------------------------------------------------------

const layerStack: string[] = [];

function useLayer(open: boolean, onClose: () => void, dismissable: boolean) {
  const id = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const dismissRef = useRef(dismissable);
  dismissRef.current = dismissable;

  useEffect(() => {
    if (!open) return;
    layerStack.push(id);
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && layerStack[layerStack.length - 1] === id && dismissRef.current) {
        e.stopPropagation();
        onCloseRef.current();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      const idx = layerStack.lastIndexOf(id);
      if (idx >= 0) layerStack.splice(idx, 1);
      if (layerStack.length === 0) document.body.style.overflow = prevOverflow;
      previouslyFocused?.focus?.();
    };
  }, [open, id]);
}

function useAutoFocus(open: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const el = ref.current;
    if (!el) return;
    const target =
      el.querySelector<HTMLElement>('[data-autofocus]') ??
      el.querySelector<HTMLElement>('input:not([disabled]),select:not([disabled]),textarea:not([disabled])') ??
      el;
    target.focus({ preventScroll: true });
  }, [open]);
  return ref;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  size = 'md',
  dismissable = true,
  icon: Icon,
}: {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  footer?: React.ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  /** False while a request is in flight. */
  dismissable?: boolean;
  icon?: React.ComponentType<{ className?: string }>;
}) {
  useLayer(open, onClose, dismissable);
  const panelRef = useAutoFocus(open);
  const titleId = useId();
  const descId = useId();
  if (!open) return null;
  const width = { sm: 'max-w-md', md: 'max-w-lg', lg: 'max-w-2xl', xl: 'max-w-4xl' }[size];
  return (
    <div
      className="fixed inset-0 z-[60] bg-slate-950/60 backdrop-blur-xs flex items-end sm:items-center justify-center p-0 sm:p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && dismissable) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descId : undefined}
        tabIndex={-1}
        className={`w-full ${width} bg-white rounded-t-3xl sm:rounded-3xl shadow-2xl border border-slate-200 flex flex-col max-h-[92vh] animate-in fade-in zoom-in-95 duration-150 focus:outline-none`}
      >
        <div className="flex items-start justify-between gap-3 px-6 pt-5 pb-4 border-b border-slate-100">
          <div className="flex items-start gap-3 min-w-0">
            {Icon && (
              <div className="w-9 h-9 rounded-xl bg-amber-50 text-amber-700 flex items-center justify-center shrink-0">
                <Icon className="w-4 h-4" />
              </div>
            )}
            <div className="min-w-0">
              <h2 id={titleId} className="text-base font-black text-slate-950 font-heading">
                {title}
              </h2>
              {description && (
                <p id={descId} className="text-xs text-slate-500 mt-0.5">
                  {description}
                </p>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={!dismissable}
            aria-label="Close"
            className="p-1.5 rounded-lg text-slate-400 hover:text-slate-900 hover:bg-slate-100 disabled:opacity-40"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
        <div className="px-6 py-5 overflow-y-auto flex-1">{children}</div>
        {footer && (
          <div className="px-6 py-4 border-t border-slate-100 flex flex-wrap items-center justify-end gap-2 bg-slate-50/60 rounded-b-3xl">
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}

export function Drawer({
  open,
  onClose,
  title,
  subtitle,
  children,
  label,
}: {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  children: React.ReactNode;
  label: string;
}) {
  useLayer(open, onClose, true);
  const panelRef = useAutoFocus(open);
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 bg-slate-950/50 backdrop-blur-xs flex justify-end"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
        className="w-full max-w-2xl h-full bg-[#F8F9FA] shadow-2xl flex flex-col animate-in slide-in-from-right duration-200 focus:outline-none"
      >
        <div className="bg-white border-b border-slate-200 px-5 sm:px-6 py-4 flex items-start justify-between gap-3">
          <div className="min-w-0">{title}{subtitle}</div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="p-1.5 rounded-lg text-slate-400 hover:text-slate-900 hover:bg-slate-100"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto">{children}</div>
      </div>
    </div>
  );
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel = 'Confirm',
  tone = 'danger',
  busy,
  error,
  onConfirm,
  onCancel,
  children,
}: {
  open: boolean;
  title: string;
  description?: React.ReactNode;
  confirmLabel?: string;
  tone?: 'danger' | 'primary';
  busy?: boolean;
  error?: string | null;
  onConfirm: () => void;
  onCancel: () => void;
  children?: React.ReactNode;
}) {
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      size="sm"
      dismissable={!busy}
      icon={AlertTriangle}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Keep as is
          </Button>
          <Button
            variant={tone === 'danger' ? 'danger' : 'primary'}
            onClick={onConfirm}
            loading={busy}
            data-autofocus
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {description && <div className="text-xs text-slate-600 leading-relaxed">{description}</div>}
      {children}
      {error && <FormError message={error} />}
    </Modal>
  );
}

export function FormError({ message }: { message: string | null | undefined }) {
  if (!message) return null;
  return (
    <div
      role="alert"
      className="mt-4 p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-xs font-bold flex items-start gap-2"
    >
      <AlertTriangle className="w-4 h-4 shrink-0 mt-px" />
      <span>{message}</span>
    </div>
  );
}

export function FieldError({ id, message }: { id: string; message?: string | null }) {
  if (!message) return null;
  return (
    <p id={id} className="mt-1 text-[11px] font-bold text-rose-600">
      {message}
    </p>
  );
}

/** Chip-style toggle button used for filters. */
export function FilterChip({
  active,
  onClick,
  children,
  count,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
  count?: number | null;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all inline-flex items-center gap-1.5 ${
        active ? 'bg-slate-950 text-white shadow-2xs' : 'bg-white text-slate-700 hover:bg-slate-100 border border-slate-200'
      }`}
    >
      {children}
      {count !== undefined && count !== null && (
        <span
          className={`text-[10px] font-black px-1.5 rounded-md tabular-nums ${
            active ? 'bg-white/15 text-white' : 'bg-slate-100 text-slate-600'
          }`}
        >
          {count}
        </span>
      )}
    </button>
  );
}
