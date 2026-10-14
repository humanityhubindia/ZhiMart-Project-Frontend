import React from 'react';
import { createPortal } from 'react-dom';
import type { GuestOrder } from '../lib/ordersApi';
import { usePricingRules } from '../hooks/useProducts';
import { pricingTierLabel } from '../lib/pricingTier';
import { NO_TAX_NOTE } from '../lib/money';
import { BUSINESS_PROFILE, formatDateTime, formatINR, instrumentLabel, numberToWords } from './admin/format';

/**
 * Print-only receipt for the customer's order page ("Print receipt" → `window.print()`).
 *
 * The global print stylesheet (index.css) hides everything except `#printable-estimation-slip`,
 * so without this the order page printed blank. The slip is portalled to <body> and, while it is
 * mounted, every other top-level node is `display: none` in print so the receipt fits on its own
 * page(s) instead of trailing blank pages for the hidden app. Invisible on screen.
 */
export const OrderReceiptPrint: React.FC<{ order: GuestOrder; statusLabel: string }> = ({ order, statusLabel }) => {
  const rules = usePricingRules();
  if (typeof document === 'undefined') return null;

  const isPaid = order.payment.status === 'paid';
  const isRefunded = order.payment.status === 'refunded';
  const isPayLater = !isPaid && !isRefunded && order.payment.method === 'pay_later';
  const instrument = instrumentLabel(order.payment.instrument);
  const paymentLabel = isRefunded
    ? 'Refunded'
    : isPaid
      ? `Paid${instrument ? ` via ${instrument}` : ''}`
      : isPayLater
        ? 'Pay later — to be confirmed with our team'
        : 'Awaiting payment';

  // Savings are on goods (subtotal) only — the delivery charge is not part of the discount.
  const discount = Math.max(0, Math.round((order.mrpTotal - order.subtotal) * 100) / 100);
  const discountPct = order.mrpTotal > 0 ? Math.round((discount / order.mrpTotal) * 100) : 0;
  // Test orders are charged ₹1/unit — no MRP savings or price tier to show.
  const tierLine = order.pricingTier && !order.isTest ? pricingTierLabel(order.pricingTier, rules) : null;
  const showMrp = !order.isTest && order.items.some((i) => (i.unitMrp ?? 0) > i.unitPrice);

  const details: [string, string][] = [
    ['Payment', paymentLabel],
    ...(order.payment.paidAt ? ([['Paid on', formatDateTime(order.payment.paidAt)]] as [string, string][]) : []),
    ...(order.shipping.transportPartner ? ([['Transport partner', order.shipping.transportPartner]] as [string, string][]) : []),
    ...(order.shipping.docketNumber ? ([['Docket / LR number', order.shipping.docketNumber]] as [string, string][]) : []),
    ...(order.shipping.dispatchedAt ? ([['Dispatched', formatDateTime(order.shipping.dispatchedAt)]] as [string, string][]) : []),
    ...(order.shipping.deliveredAt ? ([['Delivered', formatDateTime(order.shipping.deliveredAt)]] as [string, string][]) : []),
  ];

  return createPortal(
    <>
      <style>{'@media print { body > *:not(#printable-estimation-slip) { display: none !important; } }'}</style>
      <div id="printable-estimation-slip" aria-hidden="true" className="hidden print:block bg-white text-slate-900 font-sans text-xs">
        {/* Header */}
        <div className="flex items-start justify-between gap-6 pb-5 border-b-2 border-slate-900">
          <div className="flex items-start gap-3">
            <div className="w-11 h-11 rounded-xl bg-slate-950 flex items-center justify-center shrink-0">
              <span
                className="font-black text-amber-400 text-xl leading-none"
                style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
              >
                ழி
              </span>
            </div>
            <div>
              <h1 className="text-xl font-black tracking-tight font-heading leading-tight">{BUSINESS_PROFILE.name}</h1>
              <p className="text-[10px] font-extrabold uppercase tracking-wider text-amber-700">{BUSINESS_PROFILE.tagline}</p>
              <p className="text-[10px] text-slate-500 leading-relaxed mt-0.5">
                {BUSINESS_PROFILE.address}
                <br />
                Helpline: {BUSINESS_PROFILE.helpline}
              </p>
            </div>
          </div>
          <div className="text-right shrink-0">
            <p className="text-[10px] font-black uppercase tracking-wider text-slate-500">
              {isPaid ? 'Order receipt' : 'Order summary'}
            </p>
            <p className="font-mono font-black text-base mt-1">{order.orderNumber}</p>
            <p className="text-[11px] text-slate-500 font-semibold">Placed: {formatDateTime(order.createdAt)}</p>
            <p className="text-[11px] font-bold mt-0.5">Status: {statusLabel}</p>
            {order.isTest && (
              <p className="text-[10px] font-black uppercase mt-1 text-violet-800">Test order — not a real sale</p>
            )}
          </div>
        </div>

        {/* Customer / payment */}
        <div className="grid grid-cols-2 gap-6 py-5 border-b border-slate-200">
          <div>
            <p className="text-[10px] font-black uppercase tracking-wider text-slate-400 mb-1">Customer</p>
            <p className="text-sm font-black">{order.customerName}</p>
            {order.city && <p className="font-semibold text-slate-700">{order.city}</p>}
          </div>
          <dl className="space-y-1">
            {details.map(([k, v]) => (
              <div key={k} className="flex gap-2">
                <dt className="text-slate-500 font-bold shrink-0">{k}:</dt>
                <dd className="font-extrabold break-words">{v}</dd>
              </div>
            ))}
          </dl>
        </div>

        {/* Items */}
        <table className="w-full text-left border-collapse my-5">
          <thead>
            <tr className="border-b-2 border-slate-900 text-[10px] font-black uppercase tracking-wider">
              <th className="py-2 pr-2">#</th>
              <th className="py-2 px-2">Item</th>
              <th className="py-2 px-2 text-center">Qty</th>
              {showMrp && <th className="py-2 px-2 text-right">MRP</th>}
              <th className="py-2 px-2 text-right">Rate</th>
              <th className="py-2 pl-2 text-right">Amount</th>
            </tr>
          </thead>
          <tbody>
            {order.items.map((item, index) => (
              <tr key={item.productId} className="border-b border-slate-200" style={{ breakInside: 'avoid' }}>
                <td className="py-2 pr-2 font-bold text-slate-400">{index + 1}</td>
                <td className="py-2 px-2">
                  <p className="font-extrabold">{item.productName}</p>
                  {item.packInfo && <p className="text-[10px] text-slate-500">{item.packInfo}</p>}
                </td>
                <td className="py-2 px-2 text-center font-bold tabular-nums">{item.quantity}</td>
                {showMrp && (
                  <td className="py-2 px-2 text-right text-slate-400 line-through tabular-nums">
                    {item.unitMrp ? formatINR(item.unitMrp) : ''}
                  </td>
                )}
                <td className="py-2 px-2 text-right font-semibold tabular-nums">{formatINR(item.unitPrice)}</td>
                <td className="py-2 pl-2 text-right font-black tabular-nums">{formatINR(item.lineTotal)}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {/* Totals */}
        <div className="flex justify-end" style={{ breakInside: 'avoid' }}>
          <div className="w-80 space-y-1.5">
            {!order.isTest && discount > 0 && (
              <>
                <div className="flex justify-between text-slate-600">
                  <span>MRP total ({order.itemCount} units):</span>
                  <span className="tabular-nums">{formatINR(order.mrpTotal)}</span>
                </div>
                <div className="flex justify-between text-emerald-700 font-bold">
                  <span>You saved ({discountPct}%):</span>
                  <span className="tabular-nums">−{formatINR(discount)}</span>
                </div>
              </>
            )}
            {order.isTest && <p className="text-[11px] font-semibold text-slate-600">Test order — charged at ₹1 per unit.</p>}
            {tierLine && (
              <p className="text-[11px] font-semibold text-slate-700">
                <span className="font-black text-slate-900">Pricing:</span> {tierLine}
              </p>
            )}
            <div className="pt-1.5 border-t border-slate-200 flex justify-between font-bold">
              <span>Subtotal:</span>
              <span className="tabular-nums">{formatINR(order.subtotal)}</span>
            </div>
            <div className="flex justify-between text-slate-600">
              <span>Delivery charge:</span>
              <span className={order.deliveryFee > 0 ? 'tabular-nums' : 'font-black text-emerald-700'}>
                {order.deliveryFee > 0 ? formatINR(order.deliveryFee) : 'FREE'}
              </span>
            </div>
            <div className="flex justify-between text-slate-600">
              <span>Tax (GST):</span>
              <span className="tabular-nums">{formatINR(0)}</span>
            </div>
            <div className="pt-2 border-t-2 border-slate-900 flex justify-between items-baseline font-black text-sm">
              <span>{isPaid ? 'Amount paid' : isPayLater ? 'Amount payable' : 'Order total'}:</span>
              <span className="text-base tabular-nums">{formatINR(order.totalAmount)}</span>
            </div>
            <p className="text-[10px] text-slate-500 text-right">{NO_TAX_NOTE}</p>
            <p className="text-[10px] text-slate-500 italic text-right">
              Amount in words: {numberToWords(order.totalAmount)}
            </p>
          </div>
        </div>

        <div className="mt-8 pt-4 border-t border-slate-200 text-[10px] text-slate-500 leading-relaxed space-y-1">
          <p className="font-bold text-slate-700">
            Safety: always light fireworks outdoors in open ground under adult supervision.
          </p>
          <p>
            Questions about this order? Call {BUSINESS_PROFILE.helpline} and quote order number{' '}
            <span className="font-mono font-bold text-slate-700">{order.orderNumber}</span>.
          </p>
        </div>
      </div>
    </>,
    document.body
  );
};
