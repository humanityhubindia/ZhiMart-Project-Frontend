import React, { useEffect, useMemo, useState } from 'react';
import {
  ArrowLeft,
  Trash2,
  Plus,
  Minus,
  ShoppingBag,
  ShieldCheck,
  Truck,
  Check,
  MessageCircle,
  Printer,
  ArrowRight,
  AlertCircle,
  Loader2,
} from 'lucide-react';
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
import { computeTotals, formatRupees, hasBulkThreshold, lineTotal, NO_TAX_NOTE, unitPriceFor, waivedDeliveryFee } from '../lib/money';
import { StockShortage } from '../lib/api';
import { BillingLines, CartLineNotice, CheckoutFailureNotice, CheckoutFields, FreeDeliveryHint } from './CheckoutParts';
import { BulkDiscountProgress } from './BulkDiscountProgress';

interface CartPageProps {
  cartItems: CartItem[];
  /** True when any line is unavailable — checkout is blocked until it is removed. */
  hasUnavailable: boolean;
  /** Max orderable quantity for a product (live `availableQuantity`). */
  maxQuantityFor: (product: CrackerProduct) => number;
  onUpdateQuantity: (productId: string, newQty: number) => void;
  onRemoveItem: (productId: string) => void;
  onClearCart: () => void;
  onBackToShopping: () => void;
  onNavigateToProduct?: (product: CrackerProduct) => void;
  /** Called once the order exists (stock reserved) — the app opens the order page and starts payment. */
  onOrderCreated: (result: CheckoutSuccess) => void;
}

export const CartPage: React.FC<CartPageProps> = ({
  cartItems,
  hasUnavailable,
  maxQuantityFor,
  onUpdateQuantity,
  onRemoveItem,
  onClearCart,
  onBackToShopping,
  onNavigateToProduct,
  onOrderCreated,
}) => {
  const [formData, setFormData] = useState<CheckoutFormValues>(EMPTY_CHECKOUT_FORM);
  const [formErrors, setFormErrors] = useState<CheckoutErrors>({});
  const [showBlockedHint, setShowBlockedHint] = useState(false);
  const checkout = useCheckout();
  const testMode = useTestMode();
  const copy = checkoutCopy(useOnlinePaymentsEnabled());

  // Server-side validation errors about a specific field (e.g. `customer.email`) go inline on that field.
  const fieldFailure =
    checkout.failure?.kind === 'validation' && checkout.failure.field ? checkout.failure : null;
  useEffect(() => {
    if (!fieldFailure?.field) return;
    const field = fieldFailure.field;
    setFormErrors((prev) => ({ ...prev, [field]: fieldFailure.message }));
    document.getElementById(`cart-checkout-${field}`)?.focus();
  }, [fieldFailure]);

  const pricing = usePricingRules();
  const totals = useMemo(() => computeTotals(cartItems, pricing), [cartItems, pricing]);
  const {
    subtotal,
    deliveryFee,
    total: totalAmount,
    mrpTotal: totalOriginal,
    savings: totalSavings,
    units: totalItemUnits,
    discountPercent,
    bulkUnlocked,
  } = totals;
  /** Headline discount of the tier that currently applies to every line. */
  const tierPercent = bulkUnlocked ? pricing.bulkDiscountPercent : pricing.standardDiscountPercent;
  const tierLabel = bulkUnlocked ? 'Bulk price' : 'Standard price';

  const handleFieldChange = (field: CheckoutField, value: string) => {
    const next = { ...formData, [field]: value };
    setFormData(next);
    // Re-validate only fields that are currently flagged, so errors clear as the user fixes them.
    if (formErrors[field]) {
      const fieldError = validateCheckout(next)[field];
      setFormErrors((prev) => ({ ...prev, [field]: fieldError }));
    }
  };

  const handlePlaceOrder = async (e: React.FormEvent) => {
    e.preventDefault();
    if (cartItems.length === 0 || checkout.isPending) return;
    if (hasUnavailable) {
      setShowBlockedHint(true);
      return;
    }
    const errors = validateCheckout(formData);
    setFormErrors(errors);
    const firstInvalid = Object.keys(errors)[0];
    if (firstInvalid) {
      document.getElementById(`cart-checkout-${firstInvalid}`)?.focus();
      return;
    }

    const result = await checkout.submit(cartItems, formData);
    if (result) {
      // Stock is now reserved server-side for this order; payment continues on the order page.
      onClearCart();
      setFormData(EMPTY_CHECKOUT_FORM);
      setFormErrors({});
      onOrderCreated(result);
    }
  };

  const handleAdjustQuantities = (shortages: StockShortage[]) => {
    shortages.forEach((s) => onUpdateQuantity(s.productId, Math.max(0, s.available)));
    checkout.clearFailure();
  };

  const checkoutBlocked = hasUnavailable;

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8 animate-in fade-in duration-200 text-left">
      {/* 1. Header Navigation Bar */}
      <div className="flex flex-wrap items-center justify-between gap-4 pb-6 border-b border-slate-200">
        <button
          onClick={onBackToShopping}
          className="inline-flex items-center gap-2 text-sm font-bold text-slate-700 hover:text-slate-900 bg-white border border-slate-200 px-4 py-2 rounded-xl transition-all shadow-2xs hover:bg-slate-50 active:scale-97"
        >
          <ArrowLeft className="w-4 h-4" />
          <span>Continue Shopping</span>
        </button>

        <div className="flex items-center gap-2">
          {cartItems.length > 0 && (
            <button
              onClick={onClearCart}
              className="inline-flex items-center gap-1.5 text-xs font-semibold px-3.5 py-2 rounded-xl border border-rose-200 bg-rose-50/50 hover:bg-rose-100/70 text-rose-700 transition-all shadow-2xs"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>Clear Cart</span>
            </button>
          )}

          <div className="text-xs font-medium text-slate-500 bg-slate-100 px-3 py-1.5 rounded-xl">
            Sivakasi Factory Dispatch
          </div>
        </div>
      </div>

      {cartItems.length === 0 ? (
        /* 3. Empty Cart State */
        <div className="py-20 max-w-lg mx-auto text-center space-y-6">
          <div className="w-20 h-20 rounded-3xl bg-slate-100 text-slate-400 flex items-center justify-center mx-auto shadow-inner">
            <ShoppingBag className="w-10 h-10" />
          </div>
          <div className="space-y-2">
            <h2 className="text-2xl font-black text-slate-900 font-heading">
              Your Shopping Cart is Empty
            </h2>
            <p className="text-sm text-slate-500 leading-relaxed">
              Looks like you haven't added any crackers yet. Browse our Sivakasi fireworks selection
              {testMode
                ? '.'
                : hasBulkThreshold(pricing)
                  ? ` — orders of ₹${formatRupees(pricing.bulkMinOrder)}+ get ${pricing.bulkDiscountPercent}% OFF, smaller orders ${pricing.standardDiscountPercent}% OFF.`
                  : ` — ${pricing.bulkDiscountPercent}% OFF on every order.`}
            </p>
          </div>
          <button
            onClick={onBackToShopping}
            className="inline-flex items-center gap-2 bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] font-extrabold text-sm px-6 py-3.5 rounded-2xl transition-all shadow-md active:scale-97"
          >
            <ShoppingBag className="w-4 h-4" />
            <span>Explore Best Sellers</span>
          </button>
        </div>
      ) : (
        /* 4. Populated Cart Page: 2-Column Desktop Grid */
        <div className="py-6 space-y-6">
          {/* Title Header */}
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div>
              <h1 className="text-2xl sm:text-3xl font-black text-slate-900 tracking-tight font-heading">
                Shopping Cart
              </h1>
              <p className="text-xs sm:text-sm text-slate-500 font-medium">
                {cartItems.length} unique items &bull; {totalItemUnits} total units
                {!testMode && totalItemUnits > 0 && (
                  <> &bull; {tierLabel} ({tierPercent}% OFF) applied</>
                )}
              </p>
            </div>

            {testMode ? (
              <div className="flex items-center gap-2 text-xs font-semibold text-amber-800 bg-amber-50 px-3.5 py-1.5 rounded-xl border border-amber-200">
                <AlertCircle className="w-3.5 h-3.5 text-amber-600" />
                <span>Test mode — every item is charged ₹1</span>
              </div>
            ) : totalSavings > 0 ? (
              <div className="flex items-center gap-2 text-xs font-semibold text-emerald-700 bg-emerald-50 px-3.5 py-1.5 rounded-xl border border-emerald-200">
                <ShieldCheck className="w-3.5 h-3.5 text-emerald-600" />
                <span>You save ₹{formatRupees(totalSavings)} on this order!</span>
              </div>
            ) : null}
          </div>

          <BulkDiscountProgress totals={totals} onShopMore={onBackToShopping} />

          <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
            {/* Left Column: Cart Items List */}
            <div className="lg:col-span-7 space-y-4">
              <div className="bg-white border border-slate-200 rounded-3xl p-5 shadow-xs divide-y divide-slate-100">
                {cartItems.map((item) => {
                  const unitPrice = unitPriceFor(item.product, bulkUnlocked);
                  const itemTotal = lineTotal(unitPrice, item.quantity);
                  const itemSavings = Math.max(0, lineTotal(item.product.originalPrice, item.quantity) - itemTotal);
                  // While locked, show what this item would cost once the bulk tier is unlocked.
                  const bulkHint = !testMode && !bulkUnlocked && item.product.price < unitPrice;
                  const maxQty = maxQuantityFor(item.product);
                  const canIncrease = !item.unavailable && item.quantity < maxQty;
                  return (
                    <div
                      key={item.product.id}
                      className="py-4 first:pt-0 last:pb-0 flex flex-col sm:flex-row items-start sm:items-center gap-4 transition-colors"
                    >
                      {/* Product Thumbnail */}
                      <div
                        onClick={() => onNavigateToProduct && onNavigateToProduct(item.product)}
                        className={`w-20 h-20 bg-slate-50 rounded-2xl p-2 border border-slate-200 flex items-center justify-center shrink-0 cursor-pointer hover:border-slate-400 transition-colors ${
                          item.unavailable ? 'opacity-50 grayscale' : ''
                        }`}
                        title="View product details"
                      >
                        <img
                          src={item.product.image}
                          alt={item.product.name}
                          className="max-h-full max-w-full object-contain"
                        />
                      </div>

                      {/* Product Details */}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-start justify-between gap-2">
                          <div>
                            <button
                              onClick={() => onNavigateToProduct && onNavigateToProduct(item.product)}
                              className="text-sm font-bold text-slate-900 hover:text-amber-600 transition-colors text-left truncate block max-w-xs"
                            >
                              {item.product.name}
                            </button>
                            <p className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider">
                              {item.product.category}
                            </p>
                          </div>

                          <button
                            onClick={() => onRemoveItem(item.product.id)}
                            className="p-1.5 text-slate-400 hover:text-rose-600 hover:bg-rose-50 rounded-lg transition-colors"
                            title="Remove from cart"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>

                        {/* Pack info and per-item rate */}
                        <div className="flex flex-wrap items-center gap-3 mt-1 text-xs">
                          <span className="text-[11px] bg-slate-100 text-slate-600 px-2 py-0.5 rounded-md font-medium">
                            {item.product.packInfo}
                          </span>
                          <span className="text-slate-400">
                            Unit: <strong className="text-slate-700">₹{formatRupees(unitPrice)}</strong>{' '}
                            {!testMode && item.product.originalPrice > unitPrice && (
                              <span className="line-through text-slate-400">
                                ₹{formatRupees(item.product.originalPrice)}
                              </span>
                            )}
                          </span>
                          {bulkHint && (
                            <span className="text-[10px] font-bold text-orange-700 bg-orange-50 border border-orange-200 px-1.5 py-0.5 rounded-md">
                              ₹{formatRupees(item.product.price)} at {pricing.bulkDiscountPercent}% OFF
                            </span>
                          )}
                        </div>

                        <CartLineNotice item={item} className="mt-2" />

                        {/* Stepper & Subtotal row */}
                        <div className="flex items-center justify-between mt-3 pt-2 border-t border-slate-50">
                          {/* Stepper */}
                          <div className="flex items-center space-x-2 bg-slate-100 rounded-xl px-2.5 py-1">
                            <button
                              type="button"
                              onClick={() => onUpdateQuantity(item.product.id, item.quantity - 1)}
                              disabled={checkout.isPending}
                              className="p-1 text-slate-600 hover:text-black hover:bg-white rounded-lg transition-all disabled:opacity-30"
                              aria-label="Decrease quantity"
                            >
                              <Minus className="w-3.5 h-3.5" />
                            </button>
                            <span className="text-xs font-bold text-slate-900 min-w-5 text-center">
                              {item.quantity}
                            </span>
                            <button
                              type="button"
                              onClick={() => onUpdateQuantity(item.product.id, item.quantity + 1)}
                              disabled={!canIncrease || checkout.isPending}
                              title={canIncrease ? undefined : 'No more stock available'}
                              className="p-1 text-slate-600 hover:text-black hover:bg-white rounded-lg transition-all disabled:opacity-30 disabled:hover:bg-transparent disabled:cursor-not-allowed"
                              aria-label="Increase quantity"
                            >
                              <Plus className="w-3.5 h-3.5" />
                            </button>
                          </div>

                          {/* Line item subtotal */}
                          <div className="text-right">
                            <span className="text-sm font-black text-slate-900">
                              ₹{formatRupees(itemTotal)}
                            </span>
                            {!testMode && itemSavings > 0 && (
                              <span className="block text-[10px] font-semibold text-emerald-600">
                                Save ₹{formatRupees(itemSavings)}
                              </span>
                            )}
                          </div>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>

              {/* Trust Badges Bar */}
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
                  <ShieldCheck className="w-5 h-5 text-emerald-600 shrink-0" />
                  <div>
                    <p className="text-xs font-bold text-slate-800">100% Genuine</p>
                    <p className="text-[10px] text-slate-500">Direct Sivakasi stock</p>
                  </div>
                </div>

                <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
                  <Truck className="w-5 h-5 text-blue-600 shrink-0" />
                  <div>
                    <p className="text-xs font-bold text-slate-800">Insured Transport</p>
                    <p className="text-[10px] text-slate-500">Safe parcels dispatch</p>
                  </div>
                </div>

                <div className="p-3.5 bg-white border border-slate-200 rounded-2xl flex items-center gap-3">
                  <MessageCircle className="w-5 h-5 text-[#25D366] shrink-0" />
                  <div>
                    <p className="text-xs font-bold text-slate-800">WhatsApp Updates</p>
                    <p className="text-[10px] text-slate-500">Live order support</p>
                  </div>
                </div>
              </div>
            </div>

            {/* Right Column: Order Summary & Checkout Form */}
            <div className="lg:col-span-5 space-y-4 lg:sticky lg:top-24">
              {/* Order Summary Box */}
              <div className="bg-white border border-slate-200 rounded-3xl p-6 shadow-sm space-y-5">
                <h2 className="text-base font-black text-slate-900 border-b border-slate-100 pb-3 font-heading uppercase tracking-wide">
                  Order Summary
                </h2>

                {/* Last nudge before paying (the full progress card sits above the item list). */}
                {!bulkUnlocked && <BulkDiscountProgress totals={totals} variant="compact" />}

                <div className="space-y-2 text-xs">
                  {testMode ? (
                    <div className="flex items-center justify-between text-amber-700 font-bold">
                      <span>Test price ({totalItemUnits} units × ₹1)</span>
                      <span>₹{formatRupees(subtotal)}</span>
                    </div>
                  ) : (
                    <>
                      <div className="flex items-center justify-between text-slate-500 font-medium">
                        <span>Total MRP ({totalItemUnits} units)</span>
                        <span className="line-through text-slate-400">₹{formatRupees(totalOriginal)}</span>
                      </div>

                      {totalSavings > 0 && (
                        <div className="flex items-center justify-between text-emerald-600 font-bold">
                          <span>
                            {tierLabel} discount ({discountPercent}% off MRP)
                          </span>
                          <span>- ₹{formatRupees(totalSavings)}</span>
                        </div>
                      )}

                      <div className="flex items-center justify-between text-slate-600 font-medium">
                        <span>Price tier</span>
                        <span
                          className={
                            bulkUnlocked
                              ? 'text-[10px] font-extrabold uppercase tracking-wide text-emerald-700 bg-emerald-50 border border-emerald-200 px-2 py-0.5 rounded-full'
                              : 'text-[10px] font-extrabold uppercase tracking-wide text-amber-800 bg-amber-50 border border-amber-200 px-2 py-0.5 rounded-full'
                          }
                        >
                          {bulkUnlocked
                            ? `Bulk · ${pricing.bulkDiscountPercent}% OFF`
                            : `Standard · ${pricing.standardDiscountPercent}% OFF`}
                        </span>
                      </div>
                    </>
                  )}

                  <div className="flex items-center justify-between text-slate-600 font-medium">
                    <span>Packaging & Safety Box</span>
                    <span className="text-emerald-600 font-bold">FREE</span>
                  </div>

                  <BillingLines
                    subtotal={subtotal}
                    deliveryFee={deliveryFee}
                    waivedFee={waivedDeliveryFee(totals)}
                    className="border-t border-slate-100 pt-2"
                  />

                  <FreeDeliveryHint totals={totals} />

                  <div className="border-t border-slate-200 pt-3 flex items-baseline justify-between">
                    <div>
                      <span className="text-xs font-bold text-slate-500 block">Total Net Payable</span>
                      <span className="text-[11px] text-emerald-600 font-bold">{NO_TAX_NOTE}</span>
                    </div>
                    <span className="text-2xl font-black text-slate-900">
                      ₹{formatRupees(totalAmount)}
                    </span>
                  </div>
                </div>

                {/* Savings Banner */}
                {!testMode && totalSavings > 0 && (
                  <div className="bg-emerald-50 border border-emerald-200 rounded-2xl p-3 flex items-center gap-2.5 text-xs text-emerald-800">
                    <ShieldCheck className="w-4 h-4 text-emerald-600 shrink-0" />
                    <span className="font-semibold">
                      Awesome! You are saving <strong>₹{formatRupees(totalSavings)}</strong> on this festive order.
                    </span>
                  </div>
                )}

                {/* Delivery details → POST /orders → order page (Cashfree payment there when online payments are on) */}
                <form onSubmit={handlePlaceOrder} noValidate className="space-y-3 pt-2 border-t border-slate-100">
                  <h3 className="text-xs font-black text-slate-800 uppercase tracking-wide">
                    Dispatch & Shipping Address
                  </h3>

                  <CheckoutFields
                    values={formData}
                    errors={formErrors}
                    onChange={handleFieldChange}
                    disabled={checkout.isPending}
                    idPrefix="cart-checkout"
                  />

                  {checkoutBlocked && (
                    <div
                      role={showBlockedHint ? 'alert' : undefined}
                      className="rounded-2xl border border-rose-200 bg-rose-50/70 p-3 text-xs text-rose-900 flex items-start gap-2"
                    >
                      <AlertCircle className="w-4 h-4 text-rose-600 shrink-0 mt-0.5" />
                      <span className="font-semibold">
                        Some items in your cart are no longer available. Remove them to place your order.
                      </span>
                    </div>
                  )}

                  {checkout.failure && !fieldFailure && (
                    <CheckoutFailureNotice
                      failure={checkout.failure}
                      cartItems={cartItems}
                      onAdjustQuantities={handleAdjustQuantities}
                      onDismiss={checkout.clearFailure}
                    />
                  )}

                  {/* Actions */}
                  <div className="space-y-2.5 pt-3">
                    <button
                      type="submit"
                      disabled={checkout.isPending || checkoutBlocked}
                      aria-busy={checkout.isPending}
                      className="w-full bg-[#0F172A] hover:bg-slate-800 active:scale-98 text-white py-3.5 rounded-2xl text-xs font-extrabold transition-all shadow-md flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100"
                    >
                      {checkout.isPending ? (
                        <>
                          <Loader2 className="w-4 h-4 animate-spin text-amber-400" />
                          <span>{copy.pending}</span>
                        </>
                      ) : (
                        <>
                          <span>
                            {checkout.failure?.kind === 'network' ? 'Retry' : copy.action} ₹
                            {formatRupees(totalAmount)}
                          </span>
                          <ArrowRight className="w-4 h-4 text-amber-400" />
                        </>
                      )}
                    </button>

                    {copy.note && (
                      <p className="text-[10px] text-slate-500 text-center leading-relaxed">{copy.note}</p>
                    )}
                  </div>
                </form>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
