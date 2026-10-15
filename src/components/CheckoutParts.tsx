import React from 'react';
import { AlertCircle, Clock, MapPin, PackageX, Truck, WifiOff, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { CheckoutErrors, CheckoutField, CheckoutFormValues, REQUIRED_CHECKOUT_FIELDS } from '../lib/checkout';
import { CheckoutFailure } from '../hooks/useCheckout';
import { StockShortage } from '../lib/api';
import { CartItem } from '../types';
import { CartTotals, formatRupees, freeDeliveryHint } from '../lib/money';

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

/** Fields covered by the "Delivery available only in Tamil Nadu" notice. */
const ADDRESS_FIELDS: readonly CheckoutField[] = ['address', 'city', 'pincode'];

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

  const regionId = `${idPrefix}-delivery-region`;
  const field = (
    name: CheckoutField,
    label: string,
    input: (common: {
      id: string;
      value: string;
      disabled?: boolean;
      required: boolean;
      'aria-required': boolean;
      'aria-invalid': boolean;
      'aria-describedby'?: string;
      className: string;
      onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
    }) => React.ReactNode,
    hint?: string
  ) => {
    const id = `${idPrefix}-${name}`;
    const errId = `${id}-error`;
    const hintId = `${id}-hint`;
    const required = REQUIRED_CHECKOUT_FIELDS.includes(name);
    const describedBy = [
      ADDRESS_FIELDS.includes(name) ? regionId : null,
      hint ? hintId : null,
      errors[name] ? errId : null,
    ]
      .filter(Boolean)
      .join(' ');
    return (
      <div className="space-y-1">
        <label htmlFor={id} className={labelCls}>
          {label}
          {required ? (
            <span aria-hidden="true" className="text-rose-600 font-black ml-0.5">
              *
            </span>
          ) : (
            <span className="font-medium text-slate-400"> (optional)</span>
          )}
        </label>
        {input({
          id,
          value: values[name],
          disabled,
          // `noValidate` on the form keeps the browser's own bubbles off; this is for assistive tech.
          required,
          'aria-required': required,
          'aria-invalid': Boolean(errors[name]),
          'aria-describedby': describedBy || undefined,
          className: inputCls(name),
          onChange: (e) => onChange(name, e.target.value),
        })}
        {hint && (
          <p id={hintId} className="text-[10px] font-medium text-slate-400">
            {hint}
          </p>
        )}
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
      <p className="text-[10px] font-semibold text-slate-400">
        Fields marked <span className="text-rose-600 font-black">*</span> are required.
      </p>
      {field('name', 'Name', (p) => (
        <input {...p} type="text" autoComplete="name" placeholder="Enter your full name" maxLength={100} />
      ))}
      {field('phone', 'WhatsApp Mobile Number', (p) => (
        <input
          {...p}
          type="tel"
          inputMode="tel"
          autoComplete="tel"
          placeholder="Enter 10-digit mobile number"
          maxLength={20}
        />
      ))}
      {field(
        'email',
        'Email',
        (p) => (
          <input
            {...p}
            type="email"
            inputMode="email"
            autoComplete="email"
            autoCapitalize="off"
            spellCheck={false}
            placeholder="Enter your email address"
            maxLength={254}
          />
        ),
        "We'll send your order confirmation and tracking link here."
      )}
      <p
        id={regionId}
        className="flex items-center gap-1.5 text-[11px] font-bold text-amber-900 bg-amber-50 border border-amber-200 rounded-xl px-3 py-2"
      >
        <MapPin className="w-3.5 h-3.5 text-amber-600 shrink-0" aria-hidden="true" />
        Delivery available only in Tamil Nadu
      </p>
      {field('address', 'Delivery Address', (p) => (
        <textarea
          {...p}
          rows={2}
          autoComplete="street-address"
          placeholder="House no., street, area, landmark"
          maxLength={500}
          className={cn(p.className, 'resize-none')}
        />
      ))}
      <div className="grid grid-cols-2 gap-2.5">
        {field('city', 'City', (p) => (
          <input {...p} type="text" autoComplete="address-level2" placeholder="Enter your city" maxLength={100} />
        ))}
        {field('pincode', 'Pincode', (p) => (
          <input
            {...p}
            type="text"
            inputMode="numeric"
            autoComplete="postal-code"
            placeholder="Enter 6-digit pincode"
            maxLength={6}
          />
        ))}
      </div>
      {field('notes', 'Order Notes', (p) => (
        <textarea
          {...p}
          rows={2}
          placeholder="Delivery instructions, preferred dispatch date…"
          maxLength={1000}
          className={cn(p.className, 'resize-none')}
        />
      ))}
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

// ---------------------------------------------------------------------------
// Billing lines (subtotal / delivery / tax)
// ---------------------------------------------------------------------------

/** "₹500", or a green "FREE" (with the waived fee struck through when given) when no charge applies. */
export const DeliveryChargeValue: React.FC<{ fee: number; waivedFee?: number; className?: string }> = ({
  fee,
  waivedFee = 0,
  className,
}) =>
  fee > 0 ? (
    <span className={cn('font-semibold text-slate-800 tabular-nums', className)}>₹{formatRupees(fee)}</span>
  ) : (
    <span className={cn('flex items-center gap-1.5', className)}>
      {waivedFee > 0 && (
        <span className="text-slate-400 line-through tabular-nums" aria-label={`was ₹${formatRupees(waivedFee)}`}>
          ₹{formatRupees(waivedFee)}
        </span>
      )}
      <span className="font-extrabold text-emerald-600">{waivedFee > 0 ? 'FREE delivery' : 'FREE'}</span>
    </span>
  );

/**
 * The Subtotal → Delivery charge → Tax (GST) lines shown above the payable total on the cart,
 * drawer and order page. Subtotal is the goods value after tier pricing.
 */
export const BillingLines: React.FC<{
  subtotal: number;
  deliveryFee: number;
  /** Delivery fee the order would have paid; struck through next to "FREE delivery" when waived. */
  waivedFee?: number;
  className?: string;
}> = ({ subtotal, deliveryFee, waivedFee, className }) => (
  <div className={cn('space-y-1.5 text-xs', className)}>
    <div className="flex items-center justify-between text-slate-700 font-semibold">
      <span>Subtotal</span>
      <span className="tabular-nums">₹{formatRupees(subtotal)}</span>
    </div>
    <div className="flex items-center justify-between text-slate-600 font-medium">
      <span>Delivery charge</span>
      <DeliveryChargeValue fee={deliveryFee} waivedFee={waivedFee} />
    </div>
    <div className="flex items-center justify-between text-slate-600 font-medium">
      <span>Tax (GST)</span>
      <span className="font-semibold text-slate-800 tabular-nums">₹0</span>
    </div>
  </div>
);

/** "Add ₹X more for FREE delivery (free on orders ₹5,000+)" — renders nothing once delivery is free. */
export const FreeDeliveryHint: React.FC<{ totals: CartTotals; className?: string }> = ({ totals, className }) => {
  const hint = freeDeliveryHint(totals);
  if (!hint) return null;
  return (
    <p
      className={cn(
        'flex items-start gap-1.5 text-[11px] font-bold text-sky-900 bg-sky-50 border border-sky-200 rounded-xl px-2.5 py-1.5',
        className
      )}
    >
      <Truck className="w-3.5 h-3.5 text-sky-600 shrink-0 mt-px" aria-hidden="true" />
      <span>{hint}</span>
    </p>
  );
};
