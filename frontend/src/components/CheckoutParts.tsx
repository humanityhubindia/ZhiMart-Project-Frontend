import React from 'react';
import { AlertCircle, Clock, PackageX, WifiOff, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { CheckoutErrors, CheckoutField, CheckoutFormValues } from '../lib/checkout';
import { CheckoutFailure } from '../hooks/useCheckout';
import { StockShortage } from '../lib/api';
import { CartItem } from '../types';

// ---------------------------------------------------------------------------
// Form fields
// ---------------------------------------------------------------------------

interface CheckoutFieldsProps {
  values: CheckoutFormValues;
  errors: CheckoutErrors;
  onChange: (field: CheckoutField, value: string) => void;
  disabled?: boolean;
  /** `page` = cart page styling, `compact` = drawer styling. */
  variant?: 'page' | 'compact';
  idPrefix?: string;
}

export const CheckoutFields: React.FC<CheckoutFieldsProps> = ({
  values,
  errors,
  onChange,
  disabled,
  variant = 'page',
  idPrefix = 'checkout',
}) => {
  const labelCls =
    variant === 'page' ? 'text-[11px] font-bold text-slate-600' : 'text-[11px] font-bold text-slate-700';
  const inputCls = (field: CheckoutField) =>
    cn(
      'w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border outline-none transition-colors disabled:bg-slate-50 disabled:text-slate-500',
      errors[field]
        ? 'border-rose-400 focus:border-rose-600 bg-rose-50/30'
        : variant === 'page'
          ? 'border-slate-200 focus:border-slate-800'
          : 'border-slate-300 focus:border-slate-800'
    );

  const field = (
    name: CheckoutField,
    label: string,
    input: (common: {
      id: string;
      value: string;
      disabled?: boolean;
      'aria-invalid': boolean;
      'aria-describedby'?: string;
      className: string;
      onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
    }) => React.ReactNode,
    optional = false
  ) => {
    const id = `${idPrefix}-${name}`;
    const errId = `${id}-error`;
    return (
      <div className="space-y-1">
        <label htmlFor={id} className={labelCls}>
          {label}
          {optional && <span className="font-medium text-slate-400"> (optional)</span>}
        </label>
        {input({
          id,
          value: values[name],
          disabled,
          'aria-invalid': Boolean(errors[name]),
          'aria-describedby': errors[name] ? errId : undefined,
          className: inputCls(name),
          onChange: (e) => onChange(name, e.target.value),
        })}
        {errors[name] && (
          <p id={errId} className="text-[11px] font-semibold text-rose-600">
            {errors[name]}
          </p>
        )}
      </div>
    );
  };

  return (
    <div className="space-y-3">
      {field('name', 'Full Name', (p) => (
        <input {...p} type="text" autoComplete="name" placeholder="Enter recipient name" maxLength={100} />
      ))}
      {field('phone', 'WhatsApp Mobile Number', (p) => (
        <input {...p} type="tel" inputMode="tel" autoComplete="tel" placeholder="+91 98765 43210" maxLength={20} />
      ))}
      {field('email', "Email — we'll send your order confirmation and tracking link", (p) => (
        <input
          {...p}
          type="email"
          inputMode="email"
          autoComplete="email"
          autoCapitalize="off"
          spellCheck={false}
          placeholder="you@example.com"
          maxLength={254}
          required
          aria-required="true"
        />
      ))}
      {field('address', 'Delivery Address', (p) => (
        <textarea
          {...p}
          rows={2}
          autoComplete="street-address"
          placeholder="House No, Street, Landmark"
          maxLength={500}
          className={cn(p.className, 'resize-none')}
        />
      ))}
      <div className="grid grid-cols-2 gap-2.5">
        {field('city', 'City', (p) => (
          <input {...p} type="text" autoComplete="address-level2" placeholder="Chennai / Madurai" maxLength={100} />
        ))}
        {field('pincode', 'Pincode', (p) => (
          <input
            {...p}
            type="text"
            inputMode="numeric"
            autoComplete="postal-code"
            placeholder="600028"
            maxLength={6}
          />
        ))}
      </div>
      {field('state', 'State', (p) => (
        <input {...p} type="text" autoComplete="address-level1" placeholder="Tamil Nadu" maxLength={100} />
      ))}
      {field(
        'notes',
        'Order Notes',
        (p) => (
          <textarea
            {...p}
            rows={2}
            placeholder="Delivery instructions, preferred dispatch date…"
            maxLength={1000}
            className={cn(p.className, 'resize-none')}
          />
        ),
        true
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Failure notice
// ---------------------------------------------------------------------------

interface CheckoutFailureNoticeProps {
  failure: CheckoutFailure;
  cartItems: CartItem[];
  onAdjustQuantities?: (shortages: StockShortage[]) => void;
  onDismiss?: () => void;
}

export const CheckoutFailureNotice: React.FC<CheckoutFailureNoticeProps> = ({
  failure,
  cartItems,
  onAdjustQuantities,
  onDismiss,
}) => {
  const nameOf = (productId: string) =>
    cartItems.find((i) => i.product.id === productId)?.product.name ?? productId;

  const Icon =
    failure.kind === 'out_of_stock' || failure.kind === 'product_unavailable'
      ? PackageX
      : failure.kind === 'rate_limited'
        ? Clock
        : failure.kind === 'network'
          ? WifiOff
          : AlertCircle;

  return (
    <div role="alert" className="rounded-2xl border border-rose-200 bg-rose-50/70 p-3.5 text-xs text-rose-900 space-y-2.5">
      <div className="flex items-start gap-2.5">
        <Icon className="w-4 h-4 text-rose-600 shrink-0 mt-0.5" />
        <p className="font-semibold flex-1 leading-relaxed">{failure.message}</p>
        {onDismiss && (
          <button
            type="button"
            onClick={onDismiss}
            className="p-0.5 text-rose-400 hover:text-rose-700 rounded"
            aria-label="Dismiss"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      {failure.kind === 'out_of_stock' && failure.shortages.length > 0 && (
        <>
          <ul className="space-y-1 pl-6">
            {failure.shortages.map((s) => (
              <li key={s.productId} className="flex items-center justify-between gap-3">
                <span className="font-bold text-slate-800 truncate">{nameOf(s.productId)}</span>
                <span className="shrink-0 text-rose-700 font-semibold">
                  {s.available > 0
                    ? `requested ${s.requested}, only ${s.available} available`
                    : 'sold out'}
                </span>
              </li>
            ))}
          </ul>
          {onAdjustQuantities && (
            <button
              type="button"
              onClick={() => onAdjustQuantities(failure.shortages)}
              className="w-full bg-[#0F172A] hover:bg-slate-800 text-white py-2.5 rounded-xl text-xs font-extrabold transition-all shadow-xs"
            >
              Adjust quantities
            </button>
          )}
        </>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Cart line notice
// ---------------------------------------------------------------------------

/** Inline notice for a cart line that is unavailable or was auto-adjusted. */
export const CartLineNotice: React.FC<{ item: CartItem; className?: string }> = ({ item, className }) => {
  if (item.unavailable) {
    return (
      <p
        className={cn(
          'inline-flex items-center gap-1.5 text-[11px] font-bold text-rose-700 bg-rose-50 border border-rose-200 px-2 py-1 rounded-lg',
          className
        )}
      >
        <PackageX className="w-3.5 h-3.5" />
        No longer available — please remove it to continue
      </p>
    );
  }
  if (item.adjustedFrom !== undefined && item.adjustedFrom > item.quantity) {
    return (
      <p
        className={cn(
          'inline-flex items-center gap-1.5 text-[11px] font-bold text-amber-800 bg-amber-50 border border-amber-200 px-2 py-1 rounded-lg',
          className
        )}
      >
        <AlertCircle className="w-3.5 h-3.5" />
        Only {item.quantity} left — quantity adjusted from {item.adjustedFrom}
      </p>
    );
  }
  return null;
};
