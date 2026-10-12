import React, { useState } from 'react';
import { X, Plus, Minus, Trash2, ArrowRight, ShieldCheck, Check, Truck } from 'lucide-react';
import { CartItem } from '../types';

interface CartDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  cartItems: CartItem[];
  onUpdateQuantity: (productId: string, newQty: number) => void;
  onRemoveItem: (productId: string) => void;
  onClearCart: () => void;
}

export const CartDrawer: React.FC<CartDrawerProps> = ({
  isOpen,
  onClose,
  cartItems,
  onUpdateQuantity,
  onRemoveItem,
  onClearCart,
}) => {
  const [isCheckingOut, setIsCheckingOut] = useState(false);
  const [orderComplete, setOrderComplete] = useState(false);
  const [formData, setFormData] = useState({
    name: 'Customer Name',
    phone: '+91 98400 12345',
    address: 'Flat 4B, Green Towers, Gandhi Road',
    pincode: '600028',
    city: 'Chennai',
    state: 'Tamil Nadu',
    festivalDate: '2025-10-28',
  });

  if (!isOpen) return null;

  const totalAmount = cartItems.reduce(
    (acc, item) => acc + item.product.price * item.quantity,
    0
  );

  const totalOriginal = cartItems.reduce(
    (acc, item) => acc + item.product.originalPrice * item.quantity,
    0
  );

  const totalSavings = totalOriginal - totalAmount;

  const handlePlaceOrder = (e: React.FormEvent) => {
    e.preventDefault();
    setOrderComplete(true);
    setTimeout(() => {
      onClearCart();
      setOrderComplete(false);
      setIsCheckingOut(false);
      onClose();
    }, 2800);
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
              {isCheckingOut ? 'Dispatch Booking' : 'Your Shopping Bag'}
            </h2>
            <p className="text-xs text-slate-400">
              {cartItems.length} product{cartItems.length === 1 ? '' : 's'} in order
            </p>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-full hover:bg-slate-800 text-slate-400"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content Body */}
        <div className="flex-1 overflow-y-auto p-5 space-y-4">
          {orderComplete ? (
            <div className="py-20 text-center space-y-3">
              <div className="w-16 h-16 rounded-full bg-emerald-50 text-emerald-600 flex items-center justify-center mx-auto">
                <Check className="w-8 h-8" />
              </div>
              <h3 className="text-xl font-bold text-slate-900">
                Order Placed Successfully!
              </h3>
              <p className="text-xs text-slate-500 max-w-xs mx-auto leading-relaxed">
                Order #ZM-{Math.floor(100000 + Math.random() * 900000)} has been registered.
                Sivakasi factory dispatch parcel confirmation sent to your WhatsApp number.
              </p>
            </div>
          ) : isCheckingOut ? (
            <form onSubmit={handlePlaceOrder} id="checkout-form" className="space-y-3.5">
              <div className="space-y-1">
                <label className="text-[11px] font-bold text-slate-700">Full Name</label>
                <input
                  type="text"
                  required
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-300 outline-none"
                />
              </div>

              <div className="space-y-1">
                <label className="text-[11px] font-bold text-slate-700">WhatsApp Phone Number</label>
                <input
                  type="tel"
                  required
                  value={formData.phone}
                  onChange={(e) => setFormData({ ...formData, phone: e.target.value })}
                  className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-300 outline-none"
                />
              </div>

              <div className="space-y-1">
                <label className="text-[11px] font-bold text-slate-700">Delivery Address</label>
                <textarea
                  rows={2}
                  required
                  value={formData.address}
                  onChange={(e) => setFormData({ ...formData, address: e.target.value })}
                  className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-300 outline-none resize-none"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1">
                  <label className="text-[11px] font-bold text-slate-700">Pincode</label>
                  <input
                    type="text"
                    required
                    value={formData.pincode}
                    onChange={(e) => setFormData({ ...formData, pincode: e.target.value })}
                    className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-300 outline-none"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-[11px] font-bold text-slate-700">City</label>
                  <input
                    type="text"
                    required
                    value={formData.city}
                    onChange={(e) => setFormData({ ...formData, city: e.target.value })}
                    className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-300 outline-none"
                  />
                </div>
              </div>

              <div className="p-3 bg-slate-50 rounded-xl border border-slate-200 flex items-start gap-2.5 mt-2">
                <Truck className="w-5 h-5 text-amber-600 shrink-0 mt-0.5" />
                <p className="text-[11px] text-slate-600 leading-tight">
                  Doorstep direct dispatch via insured fireworks transport partners.
                </p>
              </div>
            </form>
          ) : cartItems.length === 0 ? (
            <div className="py-20 text-center space-y-2">
              <p className="text-sm font-semibold text-slate-600">
                Your cart is empty
              </p>
              <p className="text-xs text-slate-400">
                Select your favorite crackers and enjoy flat 45% discount!
              </p>
            </div>
          ) : (
            <div className="space-y-3">
              {cartItems.map((item) => (
                <div
                  key={item.product.id}
                  className="flex gap-3.5 p-3 rounded-2xl border border-neutral-200 bg-neutral-50/70"
                >
                  <div className="w-16 h-16 bg-white rounded-xl p-1 flex items-center justify-center border shrink-0">
                    <img
                      src={item.product.image}
                      alt={item.product.name}
                      className="max-h-full max-w-full object-contain"
                    />
                  </div>

                  <div className="flex-1 flex flex-col justify-between">
                    <div>
                      <div className="flex items-start justify-between">
                        <h4 className="text-xs font-bold text-slate-900 leading-tight">
                          {item.product.name}
                        </h4>
                        <button
                          onClick={() => onRemoveItem(item.product.id)}
                          className="text-slate-400 hover:text-red-600 ml-1"
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </div>
                      <p className="text-[10px] text-slate-500 font-semibold mt-0.5">
                        {item.product.packInfo}
                      </p>
                    </div>

                    <div className="flex items-center justify-between mt-2">
                      <div>
                        <span className="text-xs font-black text-slate-900">
                          ₹{item.product.price * item.quantity}
                        </span>
                        <span className="text-[10px] text-slate-400 line-through ml-1">
                          ₹{item.product.originalPrice * item.quantity}
                        </span>
                      </div>

                      <div className="flex items-center space-x-2 bg-white border border-neutral-200 rounded-lg px-2 py-0.5">
                        <button
                          onClick={() => onUpdateQuantity(item.product.id, item.quantity - 1)}
                          className="p-0.5 text-slate-600 hover:text-black"
                        >
                          <Minus className="w-3 h-3" />
                        </button>
                        <span className="text-xs font-bold text-slate-800 px-1">
                          {item.quantity}
                        </span>
                        <button
                          onClick={() => onUpdateQuantity(item.product.id, item.quantity + 1)}
                          className="p-0.5 text-slate-600 hover:text-black"
                        >
                          <Plus className="w-3 h-3" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer Totals */}
        {cartItems.length > 0 && !orderComplete && (
          <div className="p-5 border-t border-neutral-200 bg-white space-y-3">
            <div className="space-y-1 text-xs">
              <div className="flex items-center justify-between text-slate-500">
                <span>Original Total</span>
                <span className="line-through">₹{totalOriginal}</span>
              </div>
              <div className="flex items-center justify-between text-emerald-600 font-bold">
                <span>45% Festival Discount</span>
                <span>- ₹{totalSavings}</span>
              </div>
              <div className="flex items-center justify-between text-sm font-black text-slate-900 pt-1 border-t border-neutral-100">
                <span>Net Payable</span>
                <span className="text-base">₹{totalAmount}</span>
              </div>
            </div>

            {isCheckingOut ? (
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setIsCheckingOut(false)}
                  className="w-1/3 py-2.5 rounded-xl border border-slate-300 text-xs font-bold text-slate-700"
                >
                  Back
                </button>
                <button
                  type="submit"
                  form="checkout-form"
                  className="w-2/3 bg-[#0F172A] hover:bg-slate-800 text-white py-2.5 rounded-xl text-xs font-bold transition-all shadow-md"
                >
                  Confirm Order (₹{totalAmount})
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setIsCheckingOut(true)}
                className="w-full bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] py-3 rounded-xl text-xs font-bold transition-all shadow-xs flex items-center justify-center gap-2"
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
