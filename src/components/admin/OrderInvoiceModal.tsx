import React, { useEffect } from 'react';
import { MessageCircle, Printer, X } from 'lucide-react';
import type { Order } from '../../types/admin';
import { usePricingRules } from '../../hooks/useProducts';
import { pricingTierLabel } from '../../lib/pricingTier';
import {
  BUSINESS_PROFILE,
  formatDateTime,
  formatINR,
  numberToWords,
  ORDER_STATUS_META,
  PAYMENT_STATUS_META,
  paymentMethodSummary,
  whatsappLink,
} from './format';

/** Printable order estimation slip (uses the `#printable-estimation-slip` print stylesheet). */
export function OrderInvoiceModal({ order, onClose }: { order: Order | null; onClose: () => void }) {
  const pricingRules = usePricingRules();
  useEffect(() => {
    if (!order) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    // Capture so the drawer underneath doesn't also close.
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [order, onClose]);

  if (!order) return null;
  const wa = whatsappLink(order);
  const discount = Math.max(0, Math.round((order.mrpTotal - order.totalAmount) * 100) / 100);
  const discountPct = order.mrpTotal > 0 ? Math.round((discount / order.mrpTotal) * 100) : 0;
  const c = order.customer;
  // Test orders are charged ₹1/unit, so the price tier doesn't apply to them.
  const tierLine = order.pricingTier && !order.isTest ? pricingTierLabel(order.pricingTier, pricingRules) : null;

  return (
    <div
      className="fixed inset-0 z-[70] bg-slate-950/80 backdrop-blur-xs flex items-center justify-center p-2 sm:p-4 overflow-y-auto"
      role="dialog"
      aria-modal="true"
      aria-label={`Order slip ${order.orderNumber}`}
    >
      <div className="w-full max-w-4xl bg-slate-100 rounded-3xl shadow-2xl border border-slate-300 overflow-hidden my-6 flex flex-col max-h-[92vh]">
        <div className="no-print bg-slate-900 text-white px-6 py-4 flex flex-wrap items-center justify-between gap-3 border-b border-slate-800 shrink-0">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-amber-500/20 text-amber-400 flex items-center justify-center">
              <Printer className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-sm font-black text-white font-heading">Order slip • {order.orderNumber}</h3>
              <p className="text-[11px] font-semibold text-slate-400">
                {c.name} ({c.city})
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => window.print()}
              className="px-4 py-2.5 bg-amber-500 hover:bg-amber-400 text-slate-950 font-black text-xs rounded-xl transition-all shadow-sm flex items-center gap-2"
            >
              <Printer className="w-4 h-4" />
              <span>Print / Save PDF</span>
            </button>
            {wa && (
              <a
                href={wa}
                target="_blank"
                rel="noopener noreferrer"
                className="px-3.5 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white font-bold text-xs rounded-xl transition-all flex items-center gap-1.5"
              >
                <MessageCircle className="w-4 h-4" />
                <span className="hidden sm:inline">WhatsApp update</span>
              </a>
            )}
            <button
              type="button"
              onClick={onClose}
              aria-label="Close order slip"
              className="p-2 text-slate-400 hover:text-white hover:bg-slate-800 rounded-xl transition-colors"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        <div className="overflow-y-auto p-4 sm:p-8 bg-slate-200/60 flex-1">
          <div
            id="printable-estimation-slip"
            className="bg-white rounded-2xl border border-slate-300 p-6 sm:p-10 shadow-lg text-slate-900 mx-auto max-w-3xl font-sans"
          >
            {/* Header */}
            <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4 pb-6 border-b-2 border-slate-900">
              <div className="flex items-start gap-3.5">
                <div className="w-12 h-12 rounded-2xl bg-slate-950 flex items-center justify-center shrink-0 border border-amber-500/30">
                  <span
                    className="font-black text-amber-400 text-2xl leading-none"
                    style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                  >
                    ழி
                  </span>
                </div>
                <div>
                  <h1 className="text-xl sm:text-2xl font-black text-slate-950 tracking-tight font-heading leading-tight">
                    {BUSINESS_PROFILE.name}
                  </h1>
                  <p className="text-[11px] font-extrabold uppercase tracking-wider text-amber-700">
                    {BUSINESS_PROFILE.tagline}
                  </p>
                  <p className="text-[10px] text-slate-500 font-medium leading-relaxed mt-0.5 max-w-md">
                    {BUSINESS_PROFILE.address}
                    <br />
                    Helpline: {BUSINESS_PROFILE.helpline}
                    {(BUSINESS_PROFILE.gstin || BUSINESS_PROFILE.licence) && <br />}
                    {BUSINESS_PROFILE.gstin && <span className="font-bold text-slate-700">GSTIN: {BUSINESS_PROFILE.gstin}</span>}
                    {BUSINESS_PROFILE.gstin && BUSINESS_PROFILE.licence && ' • '}
                    {BUSINESS_PROFILE.licence && (
                      <span className="font-bold text-slate-700">Licence: {BUSINESS_PROFILE.licence}</span>
                    )}
                  </p>
                </div>
              </div>
              <div className="text-left sm:text-right shrink-0">
                <span className="inline-block bg-slate-900 text-amber-400 text-[10px] font-black uppercase px-2.5 py-1 rounded-md tracking-wider">
                  Order estimation
                </span>
                <p className="font-mono font-black text-base text-slate-950 mt-1.5">{order.orderNumber}</p>
                <p className="text-[11px] text-slate-500 font-semibold">Date: {formatDateTime(order.createdAt)}</p>
                <span className="inline-block mt-1 text-[10px] font-black px-2 py-0.5 rounded bg-slate-100 text-slate-800 uppercase">
                  Status: {ORDER_STATUS_META[order.status].label}
                </span>
                {order.isTest && (
                  <span className="block mt-1 text-[10px] font-black px-2 py-0.5 rounded bg-violet-100 text-violet-800 uppercase">
                    Test order — not a real sale
                  </span>
                )}
              </div>
            </div>

            {/* Customer / transport */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-6 py-6 border-b border-slate-200 text-xs">
              <div className="bg-slate-50/80 p-4 rounded-xl border border-slate-200">
                <p className="text-[10px] font-black uppercase tracking-wider text-slate-400 mb-2">Ship to</p>
                <p className="text-sm font-black text-slate-950">{c.name}</p>
                <p className="text-slate-600 font-bold mt-0.5">Phone: {c.phone}</p>
                {c.email && <p className="text-slate-600 font-semibold">Email: {c.email}</p>}
                <p className="text-slate-700 font-semibold mt-1 leading-relaxed whitespace-pre-line">{c.address}</p>
                <p className="text-slate-900 font-extrabold mt-1">
                  {c.city}, {c.state} – {c.pincode}
                </p>
              </div>
              <div className="bg-slate-50/80 p-4 rounded-xl border border-slate-200">
                <p className="text-[10px] font-black uppercase tracking-wider text-slate-400 mb-2">Transport & payment</p>
                <dl className="space-y-1">
                  {[
                    ['Transport partner', order.shipping.transportPartner ?? 'Not assigned yet'],
                    ['Docket / LR number', order.shipping.docketNumber ?? 'Not assigned yet'],
                    ['Dispatched', order.shipping.dispatchedAt ? formatDateTime(order.shipping.dispatchedAt) : '—'],
                    ['Payment method', paymentMethodSummary(order.payment)],
                    ['Payment status', PAYMENT_STATUS_META[order.payment.status].label],
                    ...(order.payment.razorpayPaymentId
                      ? [['Payment ID', order.payment.razorpayPaymentId]]
                      : order.payment.reference
                        ? [['Payment ref', order.payment.reference]]
                        : []),
                    ...(order.payment.amountRefunded > 0 ? [['Refunded', formatINR(order.payment.amountRefunded)]] : []),
                  ].map(([k, v]) => (
                    <div key={k} className="flex justify-between gap-3">
                      <dt className="text-slate-500 font-bold">{k}:</dt>
                      <dd className="font-extrabold text-slate-900 text-right break-all">{v}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            </div>

            {/* Items */}
            <div className="py-6">
              <table className="w-full text-left border-collapse text-xs">
                <thead>
                  <tr className="bg-slate-900 text-white text-[10px] font-black uppercase tracking-wider">
                    <th className="py-2.5 px-3 rounded-l-lg">#</th>
                    <th className="py-2.5 px-3">Item</th>
                    <th className="py-2.5 px-3 text-center">Qty</th>
                    <th className="py-2.5 px-3 text-right">MRP</th>
                    <th className="py-2.5 px-3 text-right">Rate</th>
                    <th className="py-2.5 px-3 text-right rounded-r-lg">Amount</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-200">
                  {order.items.map((item, index) => (
                    <tr key={item.productId}>
                      <td className="py-3 px-3 font-bold text-slate-400">{index + 1}</td>
                      <td className="py-3 px-3">
                        <p className="font-extrabold text-slate-900">{item.productName}</p>
                        <p className="text-[10px] text-slate-400">
                          {item.packInfo} · <span className="font-mono">{item.productId}</span>
                        </p>
                      </td>
                      <td className="py-3 px-3 text-center font-bold text-slate-800 tabular-nums">{item.quantity}</td>
                      <td className="py-3 px-3 text-right text-slate-400 line-through tabular-nums">{formatINR(item.unitMrp)}</td>
                      <td className="py-3 px-3 text-right font-semibold text-slate-700 tabular-nums">{formatINR(item.unitPrice)}</td>
                      <td className="py-3 px-3 text-right font-black text-slate-950 tabular-nums">{formatINR(item.lineTotal)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Totals */}
            <div className="py-4 border-t-2 border-slate-200 grid grid-cols-1 sm:grid-cols-2 gap-6 items-start text-xs">
              <div className="space-y-2 text-slate-500 text-[11px] leading-relaxed">
                {order.notes && (
                  <div>
                    <p className="font-extrabold text-slate-800 uppercase tracking-wider text-[10px]">Customer notes</p>
                    <p className="whitespace-pre-line">{order.notes}</p>
                  </div>
                )}
                {order.status === 'cancelled' && (
                  <p className="font-bold text-rose-700">
                    Cancelled {formatDateTime(order.cancelledAt)}
                    {order.cancelReason ? ` — ${order.cancelReason}` : ''}
                  </p>
                )}
                <div className="p-2.5 rounded-lg bg-amber-50 border border-amber-200/80 text-amber-900 text-[10px] font-bold">
                  Safety: always light fireworks outdoors in open ground under adult supervision.
                </div>
              </div>
              <div className="bg-slate-50 p-4 rounded-xl border border-slate-200 space-y-2">
                <div className="flex justify-between text-slate-600">
                  <span>MRP total ({order.itemCount} units):</span>
                  <span className="font-semibold text-slate-700 tabular-nums">{formatINR(order.mrpTotal)}</span>
                </div>
                {discount > 0 && (
                  <div className="flex justify-between text-emerald-700 font-bold">
                    <span>Discount ({discountPct}%):</span>
                    <span className="tabular-nums">−{formatINR(discount)}</span>
                  </div>
                )}
                {tierLine && (
                  <p className="text-[11px] text-slate-700 font-semibold leading-snug">
                    <span className="font-black text-slate-900">Pricing:</span> {tierLine}
                  </p>
                )}
                <div className="pt-2 border-t border-slate-200 flex justify-between items-center text-sm font-black text-slate-950">
                  <span>TOTAL:</span>
                  <span className="text-base text-amber-600 font-heading tabular-nums">{formatINR(order.totalAmount)}</span>
                </div>
                <p className="text-[10px] text-slate-500 font-medium pt-1 text-right italic">
                  Amount in words: {numberToWords(order.totalAmount)}
                </p>
              </div>
            </div>

            {/* Signatures */}
            <div className="pt-8 mt-6 border-t border-slate-200 flex flex-row items-end justify-between gap-4">
              <div className="text-center w-40">
                <div className="h-12 border-b border-dashed border-slate-400 mb-1" />
                <p className="text-[10px] font-bold text-slate-500 uppercase tracking-wider">Customer signature</p>
              </div>
              <div className="text-center flex flex-col items-center">
                <div className="h-12 border-b border-slate-400 w-36 mb-1" />
                <p className="text-[10px] font-black text-slate-900 uppercase tracking-wider">Authorised signatory</p>
                <p className="text-[9px] text-slate-400">{BUSINESS_PROFILE.name}</p>
              </div>
            </div>
          </div>
        </div>

        <div className="no-print bg-white px-6 py-3.5 border-t border-slate-200 flex flex-wrap items-center justify-between gap-2 shrink-0">
          <span className="text-xs text-slate-500 font-semibold">
            Tip: choose <em>Save as PDF</em> as the printer to download a copy.
          </span>
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 bg-slate-900 text-white rounded-xl text-xs font-black hover:bg-slate-800 transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
