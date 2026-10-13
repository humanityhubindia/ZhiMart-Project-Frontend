import React, { useEffect, useMemo, useState } from 'react';
import { X, Plus, Minus, Trash2, ArrowRight, Truck, Loader2 } from 'lucide-react';
import { CartItem, CrackerProduct } from '../types';
import { CheckoutSuccess, useCheckout } from '../hooks/useCheckout';
import { useOnlinePaymentsEnabled, usePricingRules, useTestMode } from '../hooks/useProducts';
import {
  checkoutCopy,
  CheckoutErrors,
  CheckoutField,
  CheckoutFormValues,
  EMPTY_CHECKOUT_FORM,
  validateCheckout,
} from '../lib/checkout';
import { computeTotals, formatRupees, lineTotal, unitPriceFor } from '../lib/money';
import { StockShortage } from '../lib/api';
import { CartLineNotice, CheckoutFailureNotice, CheckoutFields } from './CheckoutParts';
import { BulkDiscountProgress } from './BulkDiscountProgress';

interface CartDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  cartItems: CartItem[];
  /** True when any line is unavailable — checkout is blocked until it is removed. */
  hasUnavailable: boolean;
  /** Max orderable quantity for a product (live `availableQuantity`). */
  maxQuantityFor: (product: CrackerProduct) => number;
  onUpdateQuantity: (productId: string, newQty: number) => void;
  onRemoveItem: (productId: string) => void;
  onClearCart: () => void;
  /** Called once the order exists (stock reserved) — the app opens the order page and starts payment. */
  onOrderCreated: (result: CheckoutSuccess) => void;
}

export const CartDrawer: React.FC<CartDrawerProps> = ({
  isOpen,
  onClose,
  cartItems,
  hasUnavailable,
  maxQuantityFor,
  onUpdateQuantity,
  onRemoveItem,
  onClearCart,
  onOrderCreated,
}) => {
  const [isCheckingOut, setIsCheckingOut] = useState(false);
  const [formData, setFormData] = useState<CheckoutFormValues>(EMPTY_CHECKOUT_FORM);
  const [formErrors, setFormErrors] = useState<CheckoutErrors>({});
  const checkout = useCheckout();
  const testMode = useTestMode();
  const copy = checkoutCopy(useOnlinePaymentsEnabled());
  const pricing = usePricingRules();
  // Computed before the early return below so the hook order stays stable.
  const totals = useMemo(() => computeTotals(cartItems, pricing), [cartItems, pricing]);

  // Server-side validation errors about a specific field (e.g. `customer.email`) go inline on that field.
  const fieldFailure =
    checkout.failure?.kind === 'validation' && checkout.failure.field ? checkout.failure : null;
  useEffect(() => {
    if (!fieldFailure?.field) return;
    const field = fieldFailure.field;
    setFormErrors((prev) => ({ ...prev, [field]: fieldFailure.message }));
    document.getElementById(`drawer-checkout-${field}`)?.focus();
  }, [fieldFailure]);

  if (!isOpen) return null;

  const {
    total: totalAmount,
    mrpTotal: totalOriginal,
    savings: totalSavings,
    discountPercent,
    bulkUnlocked,
  } = totals;
  const tierLabel = bulkUnlocked
    ? `Bulk price (${pricing.bulkDiscountPercent}% OFF)`
    : `Standard price (${pricing.standardDiscountPercent}% OFF)`;

  const handleFieldChange = (field: CheckoutField, value: string) => {
    const next = { ...formData, [field]: value };
    setFormData(next);
    if (formErrors[field]) {
      setFormErrors((prev) => ({ ...prev, [field]: validateCheckout(next)[field] }));
    }
  };

  const handlePlaceOrder = async (e: React.FormEvent) => {
    e.preventDefault();
    if (cartItems.length === 0 || checkout.isPending || hasUnavailable) return;
    const errors = validateCheckout(formData);
    setFormErrors(errors);
    const firstInvalid = Object.keys(errors)[0];
    if (firstInvalid) {
      document.getElementById(`drawer-checkout-${firstInvalid}`)?.focus();
      return;
    }
    const result = await checkout.submit(cartItems, formData);
    if (result) {
      onClearCart();
      setFormData(EMPTY_CHECKOUT_FORM);
      setFormErrors({});
      setIsCheckingOut(false);
      onClose();
      onOrderCreated(result);
    }
  };

  const handleAdjustQuantities = (shortages: StockShortage[]) => {
    shortages.forEach((s) => onUpdateQuantity(s.productId, Math.max(0, s.available)));
    checkout.clearFailure();
    setIsCheckingOut(false);
  };

  const handleClose = () => {
    if (checkout.isPending) return;
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-black/60 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-md bg-white h-full shadow-2xl flex flex-col justify-between animate-in slide-in-from-right duration-200 border-l border-slate-200 text-left"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top Header */}
        <div className="p-5 bg-[#0F172A] text-white flex items-center justify-between">
          <div>
            <h2 className="text-base font-bold text-white">
              {isCheckingOut ? 'Delivery Details' : 'Your Shopping Bag'}
            </h2>
            <p className="text-xs text-slate-400">
              {cartItems.length} product{cartItems.length === 1 ? '' : 's'} in order
            </p>
          </div>
          <button onClick={handleClose} className="p-1.5 rounded-full hover:bg-slate-800 text-slate-400" aria-label="Close">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content Body */}
        <div className="flex-1 overflow-y-auto p-5 space-y-4">
          {isCheckingOut ? (
            <form onSubmit={handlePlaceOrder} id="checkout-form" noValidate className="space-y-3.5">
              <CheckoutFields
                values={formData}
                errors={formErrors}
                onChange={handleFieldChange}
                disabled={checkout.isPending}
                variant="compact"
                idPrefix="drawer-checkout"
              />

              {checkout.failure && !fieldFailure && (
                <CheckoutFailureNotice
                  failure={checkout.failure}
                  cartItems={cartItems}
                  onAdjustQuantities={handleAdjustQuantities}
                  onDismiss={checkout.clearFailure}
                />
              )}

              {copy.note && (
                <div className="p-3 bg-slate-50 rounded-xl border border-slate-200 flex items-start gap-2.5 mt-2">
                  <Truck className="w-5 h-5 text-amber-600 shrink-0 mt-0.5" />
                  <p className="text-[11px] text-slate-600 leading-tight">{copy.note}</p>
                </div>
              )}
            </form>
          ) : cartItems.length === 0 ? (
            <div className="py-20 text-center space-y-2">
              <p className="text-sm font-semibold text-slate-600">Your cart is empty</p>
              <p className="text-xs text-slate-400">Select your favorite crackers from the catalog.</p>
            </div>
          ) : (
            <div className="space-y-3">
              {hasUnavailable && (
                <p className="text-[11px] font-bold text-rose-700 bg-rose-50 border border-rose-200 rounded-xl px-3 py-2">
                  Some items are no longer available. Remove them to continue to checkout.
                </p>
              )}
              {cartItems.map((item) => {
                const canIncrease = !item.unavailable && item.quantity < maxQuantityFor(item.product);
                const unitPrice = unitPriceFor(item.product, bulkUnlocked);
                const bulkHint = !testMode && !bulkUnlocked && item.product.price < unitPrice;
                return (
                  <div
                    key={item.product.id}
                    className="flex gap-3.5 p-3 rounded-2xl border border-neutral-200 bg-neutral-50/70"
                  >
                    <div className="w-16 h-16 bg-white rounded-xl p-1 flex items-center justify-center border shrink-0">
                      <img
                        src={item.product.image}
                        alt={item.product.name}
                        className={`max-h-full max-w-full object-contain ${item.unavailable ? 'opacity-50 grayscale' : ''}`}
                      />
                    </div>

                    <div className="flex-1 flex flex-col justify-between">
                      <div>
                        <div className="flex items-start justify-between">
                          <h4 className="text-xs font-bold text-slate-900 leading-tight">{item.product.name}</h4>
                          <button
                            onClick={() => onRemoveItem(item.product.id)}
                            className="text-slate-400 hover:text-red-600 ml-1"
                            aria-label="Remove from cart"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                        <p className="text-[10px] text-slate-500 font-semibold mt-0.5">
                          {item.product.packInfo} &bull; ₹{formatRupees(unitPrice)} each
                          {bulkHint && (
                            <span className="text-orange-700 font-bold">
                              {' '}
                              (₹{formatRupees(item.product.price)} at {pricing.bulkDiscountPercent}%)
                            </span>
                          )}
                        </p>
                        <CartLineNotice item={item} className="mt-1.5" />
                      </div>

                      <div className="flex items-center justify-between mt-2">
                        <div>
                          <span className="text-xs font-black text-slate-900">
                            ₹{formatRupees(lineTotal(unitPrice, item.quantity))}
                          </span>
                          {!testMode && item.product.originalPrice > unitPrice && (
                            <span className="text-[10px] text-slate-400 line-through ml-1">
                              ₹{formatRupees(lineTotal(item.product.originalPrice, item.quantity))}
                            </span>
                          )}
                        </div>

                        <div className="flex items-center space-x-2 bg-white border border-neutral-200 rounded-lg px-2 py-0.5">
                          <button
                            onClick={() => onUpdateQuantity(item.product.id, item.quantity - 1)}
                            className="p-0.5 text-slate-600 hover:text-black"
                            aria-label="Decrease quantity"
                          >
                            <Minus className="w-3 h-3" />
                          </button>
                          <span className="text-xs font-bold text-slate-800 px-1">{item.quantity}</span>
                          <button
                            onClick={() => onUpdateQuantity(item.product.id, item.quantity + 1)}
                            disabled={!canIncrease}
                            className="p-0.5 text-slate-600 hover:text-black disabled:opacity-30 disabled:cursor-not-allowed"
                            aria-label="Increase quantity"
                          >
                            <Plus className="w-3 h-3" />
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Footer Totals */}
        {cartItems.length > 0 && (
          <div className="p-5 border-t border-neutral-200 bg-white space-y-3">
            <BulkDiscountProgress totals={totals} variant="compact" />
            <div className="space-y-1 text-xs">
              {testMode ? (
                <p className="text-amber-700 font-bold">Test mode — every item is charged ₹1</p>
              ) : totalSavings > 0 ? (
                <>
                  <div className="flex items-center justify-between text-slate-500">
                    <span>Original Total</span>
                    <span className="line-through">₹{formatRupees(totalOriginal)}</span>
                  </div>
                  <div className="flex items-center justify-between text-emerald-600 font-bold">
                    <span>
                      {tierLabel} &bull; {discountPercent}% off MRP
                    </span>
                    <span>- ₹{formatRupees(totalSavings)}</span>
                  </div>
                </>
              ) : null}
              <div className="flex items-center justify-between text-sm font-black text-slate-900 pt-1 border-t border-neutral-100">
                <span>Net Payable</span>
                <span className="text-base">₹{formatRupees(totalAmount)}</span>
              </div>
            </div>

            {isCheckingOut ? (
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setIsCheckingOut(false)}
                  disabled={checkout.isPending}
                  className="w-1/3 py-2.5 rounded-xl border border-slate-300 text-xs font-bold text-slate-700 disabled:opacity-50"
                >
                  Back
                </button>
                <button
                  type="submit"
                  form="checkout-form"
                  disabled={checkout.isPending || hasUnavailable}
                  aria-busy={checkout.isPending}
                  className="w-2/3 bg-[#0F172A] hover:bg-slate-800 text-white py-2.5 rounded-xl text-xs font-bold transition-all shadow-md flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {checkout.isPending ? (
                    <>
                      <Loader2 className="w-4 h-4 animate-spin" />
                      <span>{copy.pending}</span>
                    </>
                  ) : (
                    <span>{copy.action} ₹{formatRupees(totalAmount)}</span>
                  )}
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setIsCheckingOut(true)}
                disabled={hasUnavailable}
                className="w-full bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] py-3 rounded-xl text-xs font-bold transition-all shadow-xs flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <span>Proceed to Checkout</span>
                <ArrowRight className="w-4 h-4" />
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
