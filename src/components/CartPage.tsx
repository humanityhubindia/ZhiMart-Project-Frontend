import React, { useState } from 'react';
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
  AlertCircle
} from 'lucide-react';
import { CartItem, CrackerProduct } from '../types';

interface CartPageProps {
  cartItems: CartItem[];
  onUpdateQuantity: (productId: string, newQty: number) => void;
  onRemoveItem: (productId: string) => void;
  onClearCart: () => void;
  onBackToShopping: () => void;
  onNavigateToProduct?: (product: CrackerProduct) => void;
}

export const CartPage: React.FC<CartPageProps> = ({
  cartItems,
  onUpdateQuantity,
  onRemoveItem,
  onClearCart,
  onBackToShopping,
  onNavigateToProduct,
}) => {
  const [formData, setFormData] = useState({
    name: 'Customer Name',
    phone: '+91 98400 12345',
    address: 'Flat 4B, Green Towers, Gandhi Road',
    city: 'Chennai',
    state: 'Tamil Nadu',
    pincode: '600028',
    notes: 'Diwali Festival Order',
  });

  const [orderSubmitted, setOrderSubmitted] = useState(false);
  const [orderId, setOrderId] = useState('');
  const [submittedCartItems, setSubmittedCartItems] = useState<CartItem[]>([]);
  const [submittedTotal, setSubmittedTotal] = useState(0);

  const totalAmount = cartItems.reduce(
    (acc, item) => acc + item.product.price * item.quantity,
    0
  );

  const totalOriginal = cartItems.reduce(
    (acc, item) => acc + item.product.originalPrice * item.quantity,
    0
  );

  const totalSavings = totalOriginal - totalAmount;
  const totalItemUnits = cartItems.reduce((acc, item) => acc + item.quantity, 0);

  const handlePlaceOrder = (e: React.FormEvent) => {
    e.preventDefault();
    if (cartItems.length === 0) return;

    const generatedId = `ZM-${Math.floor(100000 + Math.random() * 900000)}`;
    setOrderId(generatedId);
    setSubmittedCartItems([...cartItems]);
    setSubmittedTotal(totalAmount);
    setOrderSubmitted(true);
    onClearCart();
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleWhatsAppOrder = () => {
    if (cartItems.length === 0) return;

    const itemsSummary = cartItems
      .map(
        (i, idx) =>
          `${idx + 1}. *${i.product.name}* (${i.product.packInfo}) x ${i.quantity} = ₹${
            i.product.price * i.quantity
          }`
      )
      .join('\n');

    const message = `*ZHI-MART FACTORY DIRECT DISPATCH ORDER*\n` +
      `---------------------------------\n` +
      `*Customer:* ${formData.name}\n` +
      `*Phone:* ${formData.phone}\n` +
      `*Address:* ${formData.address}, ${formData.city} - ${formData.pincode}, ${formData.state}\n` +
      `*Notes:* ${formData.notes}\n\n` +
      `*ORDER ITEMS:*\n${itemsSummary}\n\n` +
      `---------------------------------\n` +
      `*Original MRP:* ₹${totalOriginal}\n` +
      `*Festival Discount (45%):* -₹${totalSavings}\n` +
      `*NET PAYABLE AMOUNT:* ₹${totalAmount}\n` +
      `---------------------------------\n` +
      `Please confirm dispatch availability & payment instructions.`;

    const encoded = encodeURIComponent(message);
    window.open(`https://wa.me/919840012345?text=${encoded}`, '_blank');
  };

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
          {cartItems.length > 0 && !orderSubmitted && (
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

      {/* 2. Order Placed Success View */}
      {orderSubmitted ? (
        <div className="py-12 max-w-2xl mx-auto text-center space-y-6 animate-in zoom-in-95 duration-200">
          <div className="w-20 h-20 rounded-3xl bg-emerald-100 text-emerald-600 flex items-center justify-center mx-auto shadow-sm">
            <Check className="w-10 h-10" />
          </div>

          <div className="space-y-2">
            <span className="text-xs font-extrabold uppercase tracking-wider text-emerald-700 bg-emerald-50 px-3 py-1 rounded-full border border-emerald-200">
              Order Confirmed
            </span>
            <h1 className="text-3xl font-black text-slate-900 font-heading">
              Thank You! Your Order is Placed
            </h1>
            <p className="text-sm text-slate-600 max-w-md mx-auto">
              Your booking reference is <strong className="text-slate-900 font-mono">{orderId}</strong>.
              We are packaging your order directly at our Sivakasi fireworks unit.
            </p>
          </div>

          <div className="p-6 bg-white border border-slate-200 rounded-3xl shadow-sm text-left space-y-4">
            <div className="flex items-center justify-between border-b border-slate-100 pb-3">
              <span className="text-xs font-bold text-slate-500 uppercase tracking-wider">
                Order Summary ({submittedCartItems.length} items)
              </span>
              <span className="text-base font-black text-slate-900">
                ₹{submittedTotal}
              </span>
            </div>

            <div className="space-y-2 max-h-52 overflow-y-auto pr-2 divide-y divide-slate-100">
              {submittedCartItems.map((item) => (
                <div key={item.product.id} className="pt-2 flex items-center justify-between text-xs">
                  <div>
                    <p className="font-bold text-slate-800">{item.product.name}</p>
                    <p className="text-[10px] text-slate-400 font-medium">
                      {item.product.packInfo} &bull; Qty: {item.quantity}
                    </p>
                  </div>
                  <span className="font-bold text-slate-700">
                    ₹{item.product.price * item.quantity}
                  </span>
                </div>
              ))}
            </div>

            <div className="border-t border-slate-100 pt-3 text-xs text-slate-500 space-y-1">
              <p><strong>Deliver To:</strong> {formData.name}, {formData.phone}</p>
              <p><strong>Address:</strong> {formData.address}, {formData.city} - {formData.pincode}</p>
            </div>
          </div>

          <div className="flex flex-col sm:flex-row items-center justify-center gap-3 pt-4">
            <button
              onClick={() => window.print()}
              className="inline-flex items-center gap-2 text-xs font-bold px-5 py-3 rounded-xl border border-slate-300 bg-white hover:bg-slate-50 text-slate-700 transition-all shadow-2xs"
            >
              <Printer className="w-4 h-4" />
              <span>Print Order Receipt</span>
            </button>
            <button
              onClick={() => {
                setOrderSubmitted(false);
                onBackToShopping();
              }}
              className="inline-flex items-center gap-2 text-xs font-extrabold px-6 py-3 rounded-xl bg-[#0F172A] hover:bg-slate-800 text-white transition-all shadow-md active:scale-97"
            >
              <ShoppingBag className="w-4 h-4 text-amber-400" />
              <span>Back to Catalog</span>
            </button>
          </div>
        </div>
      ) : cartItems.length === 0 ? (
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
              Looks like you haven't added any crackers yet. Browse our Sivakasi fireworks selection with flat 45% discount!
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
                {cartItems.length} unique items &bull; {totalItemUnits} total units &bull; Flat 45% Festive Discount Applied
              </p>
            </div>

            <div className="flex items-center gap-2 text-xs font-semibold text-emerald-700 bg-emerald-50 px-3.5 py-1.5 rounded-xl border border-emerald-200">
              <ShieldCheck className="w-3.5 h-3.5 text-emerald-600" />
              <span>You save ₹{totalSavings} on this order!</span>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
            {/* Left Column: Cart Items List */}
            <div className="lg:col-span-7 space-y-4">
              <div className="bg-white border border-slate-200 rounded-3xl p-5 shadow-xs divide-y divide-slate-100">
                {cartItems.map((item) => {
                  const itemSavings = (item.product.originalPrice - item.product.price) * item.quantity;
                  return (
                    <div
                      key={item.product.id}
                      className="py-4 first:pt-0 last:pb-0 flex flex-col sm:flex-row items-start sm:items-center gap-4 transition-colors"
                    >
                      {/* Product Thumbnail */}
                      <div
                        onClick={() => onNavigateToProduct && onNavigateToProduct(item.product)}
                        className="w-20 h-20 bg-slate-50 rounded-2xl p-2 border border-slate-200 flex items-center justify-center shrink-0 cursor-pointer hover:border-slate-400 transition-colors"
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
                            Unit: <strong className="text-slate-700">₹{item.product.price}</strong>{' '}
                            <span className="line-through text-slate-400">₹{item.product.originalPrice}</span>
                          </span>
                        </div>

                        {/* Stepper & Subtotal row */}
                        <div className="flex items-center justify-between mt-3 pt-2 border-t border-slate-50">
                          {/* Stepper */}
                          <div className="flex items-center space-x-2 bg-slate-100 rounded-xl px-2.5 py-1">
                            <button
                              type="button"
                              onClick={() => onUpdateQuantity(item.product.id, item.quantity - 1)}
                              className="p-1 text-slate-600 hover:text-black hover:bg-white rounded-lg transition-all"
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
                              className="p-1 text-slate-600 hover:text-black hover:bg-white rounded-lg transition-all"
                              aria-label="Increase quantity"
                            >
                              <Plus className="w-3.5 h-3.5" />
                            </button>
                          </div>

                          {/* Line item subtotal */}
                          <div className="text-right">
                            <span className="text-sm font-black text-slate-900">
                              ₹{item.product.price * item.quantity}
                            </span>
                            <span className="block text-[10px] font-semibold text-emerald-600">
                              Save ₹{itemSavings}
                            </span>
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

                <div className="space-y-2 text-xs">
                  <div className="flex items-center justify-between text-slate-500 font-medium">
                    <span>Total MRP ({totalItemUnits} units)</span>
                    <span className="line-through text-slate-400">₹{totalOriginal}</span>
                  </div>

                  <div className="flex items-center justify-between text-emerald-600 font-bold">
                    <span>Festival Special Discount (45%)</span>
                    <span>- ₹{totalSavings}</span>
                  </div>

                  <div className="flex items-center justify-between text-slate-600 font-medium">
                    <span>Packaging & Safety Box</span>
                    <span className="text-emerald-600 font-bold">FREE</span>
                  </div>

                  <div className="flex items-center justify-between text-slate-600 font-medium">
                    <span>Dispatch Transport</span>
                    <span className="text-slate-800 font-semibold">Factory Direct</span>
                  </div>

                  <div className="border-t border-slate-200 pt-3 flex items-baseline justify-between">
                    <div>
                      <span className="text-xs font-bold text-slate-500 block">Total Net Payable</span>
                      <span className="text-[11px] text-emerald-600 font-bold">Inclusive of all taxes</span>
                    </div>
                    <span className="text-2xl font-black text-slate-900">
                      ₹{totalAmount}
                    </span>
                  </div>
                </div>

                {/* Savings Banner */}
                <div className="bg-emerald-50 border border-emerald-200 rounded-2xl p-3 flex items-center gap-2.5 text-xs text-emerald-800">
                  <ShieldCheck className="w-4 h-4 text-emerald-600 shrink-0" />
                  <span className="font-semibold">
                    Awesome! You are saving <strong>₹{totalSavings}</strong> on this festive order.
                  </span>
                </div>

                {/* Delivery Booking Form */}
                <form onSubmit={handlePlaceOrder} className="space-y-3 pt-2 border-t border-slate-100">
                  <h3 className="text-xs font-black text-slate-800 uppercase tracking-wide">
                    Dispatch & Shipping Address
                  </h3>

                  <div className="space-y-1">
                    <label className="text-[11px] font-bold text-slate-600">Full Name</label>
                    <input
                      type="text"
                      required
                      value={formData.name}
                      onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                      placeholder="Enter recipient name"
                      className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-200 focus:border-slate-800 outline-none transition-colors"
                    />
                  </div>

                  <div className="space-y-1">
                    <label className="text-[11px] font-bold text-slate-600">WhatsApp Mobile Number</label>
                    <input
                      type="tel"
                      required
                      value={formData.phone}
                      onChange={(e) => setFormData({ ...formData, phone: e.target.value })}
                      placeholder="+91 98765 43210"
                      className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-200 focus:border-slate-800 outline-none transition-colors"
                    />
                  </div>

                  <div className="space-y-1">
                    <label className="text-[11px] font-bold text-slate-600">Delivery Address</label>
                    <textarea
                      rows={2}
                      required
                      value={formData.address}
                      onChange={(e) => setFormData({ ...formData, address: e.target.value })}
                      placeholder="House No, Street, Landmark"
                      className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-200 focus:border-slate-800 outline-none resize-none transition-colors"
                    />
                  </div>

                  <div className="grid grid-cols-2 gap-2.5">
                    <div className="space-y-1">
                      <label className="text-[11px] font-bold text-slate-600">City</label>
                      <input
                        type="text"
                        required
                        value={formData.city}
                        onChange={(e) => setFormData({ ...formData, city: e.target.value })}
                        placeholder="Chennai / Madurai"
                        className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-200 focus:border-slate-800 outline-none transition-colors"
                      />
                    </div>

                    <div className="space-y-1">
                      <label className="text-[11px] font-bold text-slate-600">Pincode</label>
                      <input
                        type="text"
                        required
                        value={formData.pincode}
                        onChange={(e) => setFormData({ ...formData, pincode: e.target.value })}
                        placeholder="600028"
                        className="w-full text-xs font-semibold px-3.5 py-2.5 rounded-xl border border-slate-200 focus:border-slate-800 outline-none transition-colors"
                      />
                    </div>
                  </div>

                  {/* Actions */}
                  <div className="space-y-2.5 pt-3">
                    <button
                      type="submit"
                      className="w-full bg-[#0F172A] hover:bg-slate-800 active:scale-98 text-white py-3.5 rounded-2xl text-xs font-extrabold transition-all shadow-md flex items-center justify-center gap-2"
                    >
                      <span>Confirm & Book Dispatch (₹{totalAmount})</span>
                      <ArrowRight className="w-4 h-4 text-amber-400" />
                    </button>

                    <button
                      type="button"
                      onClick={handleWhatsAppOrder}
                      className="w-full bg-[#25D366] hover:bg-[#20ba5a] active:scale-98 text-white py-3 rounded-2xl text-xs font-bold transition-all shadow-2xs flex items-center justify-center gap-2"
                    >
                      <MessageCircle className="w-4 h-4" />
                      <span>Order Directly via WhatsApp</span>
                    </button>
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
