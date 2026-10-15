import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  ArrowLeft,
  BadgePercent,
  Bookmark,
  Check,
  Clock,
  Copy,
  CreditCard,
  Link2Off,
  Loader2,
  Mail,
  MessageCircle,
  Package,
  PackageX,
  Printer,
  RotateCcw,
  ShieldCheck,
  ShoppingBag,
  Truck,
  X,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { isApiError } from '../lib/api';
import { fetchGuestOrder, GuestOrder, PricingTier } from '../lib/ordersApi';
import { buildOrderShareMessage, maskEmail, openWhatsApp } from '../lib/checkout';
import { formatRupees, hasDeliveryCharge, NO_TAX_NOTE, sumRupees } from '../lib/money';
import { useOrderPayment } from '../hooks/useOrderPayment';
import { STOREFRONT_CONFIG_QUERY_KEY, useOnlinePaymentsEnabled, usePricingRules } from '../hooks/useProducts';
import { CatalogErrorCard, errorMessage } from './CatalogStates';
import { OrderReceiptPrint } from './OrderReceipt';
import { BillingLines } from './CheckoutParts';

interface OrderPageProps {
  orderNumber: string;
  /** Guest token from the URL fragment (`#t=`). Never put it anywhere else. */
  orderToken: string | null;
  /** Open Razorpay Checkout as soon as the order loads (right after checkout). */
  autoPay?: boolean;
  /** Email typed at checkout in this session (memory only) — shown masked. */
  confirmationEmail?: string | null;
  /** What the cart showed at checkout in this session (memory only). The server's figures win. */
  clientEstimate?: { total: number; pricingTier: PricingTier } | null;
  onShopAgain: () => void;
}

const PAID_FLOW = ['pending_pack', 'dispatched', 'in_transit', 'out_for_delivery', 'delivered'] as const;

const STATUS_LABELS: Record<string, string> = {
  pending_payment: 'Awaiting payment',
  pending_pack: 'Paid — being packed',
  dispatched: 'Dispatched',
  in_transit: 'In transit',
  out_for_delivery: 'Out for delivery',
  delivered: 'Delivered',
  cancelled: 'Cancelled',
};

const STEP_LABELS: Record<(typeof PAID_FLOW)[number], string> = {
  pending_pack: 'Packing',
  dispatched: 'Dispatched',
  in_transit: 'In transit',
  out_for_delivery: 'Out for delivery',
  delivered: 'Delivered',
};

const INSTRUMENT_LABELS: Record<string, string> = {
  upi: 'UPI',
  card: 'Card',
  netbanking: 'Net Banking',
  wallet: 'Wallet',
  emi: 'EMI',
  paylater: 'Pay Later (via Razorpay)',
  bank_transfer: 'Bank Transfer',
};

const instrumentLabel = (instrument: string | null) =>
  instrument ? INSTRUMENT_LABELS[instrument] ?? instrument.replace(/_/g, ' ') : null;

const formatDateTime = (iso: string | null | undefined) => {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? null
    : d.toLocaleString('en-IN', { dateStyle: 'medium', timeStyle: 'short' });
};

function formatCountdown(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** Re-renders every second while `active`. */
function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}

export const guestOrderQueryKey = (orderNumber: string) => ['guest-order', orderNumber] as const;

export const OrderPage: React.FC<OrderPageProps> = ({
  orderNumber,
  orderToken,
  autoPay,
  confirmationEmail,
  clientEstimate,
  onShopAgain,
}) => {
  const queryClient = useQueryClient();
  const pricing = usePricingRules();
  const queryKey = guestOrderQueryKey(orderNumber);
  const [linkCopied, setLinkCopied] = useState(false);

  const refresh = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: guestOrderQueryKey(orderNumber) });
  }, [queryClient, orderNumber]);

  const onOrderUpdate = useCallback(
    (order: GuestOrder) => queryClient.setQueryData(guestOrderQueryKey(orderNumber), order),
    [queryClient, orderNumber]
  );

  // Online payments can be switched off server-side (`RAZORPAY_ENABLED=false`): orders are then
  // "pay later" and Razorpay is never loaded.
  const onlinePaymentsEnabled = useOnlinePaymentsEnabled();
  const [disabledByServer, setDisabledByServer] = useState(false);
  const paymentsOff = disabledByServer || onlinePaymentsEnabled === false;

  const onPaymentsDisabled = useCallback(() => {
    setDisabledByServer(true);
    void queryClient.invalidateQueries({ queryKey: STOREFRONT_CONFIG_QUERY_KEY });
    void queryClient.invalidateQueries({ queryKey: guestOrderQueryKey(orderNumber) });
  }, [queryClient, orderNumber]);

  const payment = useOrderPayment({ orderNumber, orderToken, onOrderUpdate, onRefresh: refresh, onPaymentsDisabled });
  const { settle, pay } = payment;
  const confirming = payment.phase === 'confirming';

  const orderQuery = useQuery<GuestOrder>({
    queryKey,
    queryFn: ({ signal }) => fetchGuestOrder(orderNumber, orderToken as string, signal),
    enabled: Boolean(orderToken && orderNumber),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: (count, err) => !(isApiError(err) && err.status >= 400 && err.status < 500) && count < 3,
    // Poll fast while the webhook settles a payment; slowly while simply awaiting payment.
    refetchInterval: (query) => {
      if (confirming) return 3_000;
      return query.state.data?.status === 'pending_payment' && !paymentsOff ? 30_000 : false;
    },
  });
  const order = orderQuery.data;
  const isPendingPayment = order?.status === 'pending_payment';
  /** Awaiting online payment, and online payments are available. */
  const awaitingOnlinePayment = isPendingPayment && !paymentsOff;

  // Once the server no longer shows the order as awaiting payment, stop any confirming state.
  useEffect(() => {
    if (order && order.status !== 'pending_payment') settle();
  }, [order, settle]);

  // Countdown to the reservation expiry.
  const now = useNow(Boolean(awaitingOnlinePayment && order?.paymentExpiresAt));
  const expiresAtMs = order?.paymentExpiresAt ? Date.parse(order.paymentExpiresAt) : NaN;
  const msLeft = Number.isFinite(expiresAtMs) ? expiresAtMs - now : null;
  const locallyExpired = awaitingOnlinePayment && msLeft !== null && msLeft <= 0;

  // When the countdown runs out, ask the server (it releases the reservation).
  const expiryRefreshed = useRef(false);
  useEffect(() => {
    if (locallyExpired && !expiryRefreshed.current) {
      expiryRefreshed.current = true;
      refresh();
    }
  }, [locallyExpired, refresh]);

  // Right after checkout: open Razorpay once, as soon as the order is known to be payable.
  const autoPayDone = useRef(false);
  useEffect(() => {
    if (!autoPay || autoPayDone.current || !order) return;
    autoPayDone.current = true;
    if (order.status === 'pending_payment' && !locallyExpired && !paymentsOff) void pay();
  }, [autoPay, order, locallyExpired, paymentsOff, pay]);

  const copyLink = async () => {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setLinkCopied(true);
      window.setTimeout(() => setLinkCopied(false), 2500);
    } catch {
      // Clipboard blocked — the link is still in the address bar.
    }
  };

  // ------------------------------------------------------------------------
  // Missing token / loading / error
  // ------------------------------------------------------------------------
  if (!orderToken || !orderNumber) {
    return (
      <PageShell onShopAgain={onShopAgain}>
        <StateCard
          icon={<Link2Off className="w-10 h-10" />}
          title="This order link is incomplete"
          body="Order pages can only be opened with the full link you received after checkout (it includes a private access key after the # sign). Please open the complete link, or contact us on WhatsApp with your order number."
          action={<ShopAgainButton onClick={onShopAgain} />}
        />
      </PageShell>
    );
  }

  if (orderQuery.isPending) {
    return (
      <PageShell onShopAgain={onShopAgain}>
        <div className="py-20 flex flex-col items-center gap-3 text-slate-500" aria-busy="true">
          <Loader2 className="w-8 h-8 animate-spin text-amber-500" />
          <p className="text-sm font-semibold">Loading your order…</p>
        </div>
      </PageShell>
    );
  }

  if (orderQuery.isError && !order) {
    const notFound = isApiError(orderQuery.error) && orderQuery.error.status === 404;
    return (
      <PageShell onShopAgain={onShopAgain}>
        {notFound ? (
          <StateCard
            icon={<PackageX className="w-10 h-10" />}
            title="Order not found"
            body="We couldn't find this order. Please make sure you opened the complete order link — it is case-sensitive and includes everything after the # sign."
            action={<ShopAgainButton onClick={onShopAgain} />}
          />
        ) : (
          <CatalogErrorCard
            className="max-w-xl mx-auto"
            title="We couldn't load your order"
            message={errorMessage(orderQuery.error)}
            onRetry={() => void orderQuery.refetch()}
            isRetrying={orderQuery.isFetching}
          />
        )}
      </PageShell>
    );
  }

  if (!order) return null;

  // ------------------------------------------------------------------------
  // Derived display state
  // ------------------------------------------------------------------------
  const isCancelled = order.status === 'cancelled';
  const isRefunded = order.payment.status === 'refunded';
  const isPaidFlow = (PAID_FLOW as readonly string[]).includes(order.status);
  const busy = payment.phase === 'preparing' || payment.phase === 'open' || payment.phase === 'verifying';
  const isPaid = order.payment.status === 'paid';
  const showConfirming =
    awaitingOnlinePayment && (payment.phase === 'verifying' || payment.phase === 'confirming');
  // Pay-later: placed without online payment; our team confirms payment and dispatch by phone.
  const isPayLater =
    !isPaid &&
    !isRefunded &&
    !isCancelled &&
    (order.payment.method === 'pay_later' || (isPendingPayment && paymentsOff) || (!isPendingPayment && !order.paymentExpiresAt));
  const expiredUnpaid =
    (isCancelled && !isPaid && !isRefunded && order.payment.method !== 'pay_later') || locallyExpired;
  const statusLabel = expiredUnpaid
    ? 'Payment window expired'
    : isPayLater && (order.status === 'pending_pack' || isPendingPayment)
      ? 'Order received'
      : STATUS_LABELS[order.status] ?? order.status;

  // Savings are on goods only — the delivery fee is not a discount.
  const goodsSavings = Math.max(0, sumRupees([order.mrpTotal, -order.subtotal]));

  const shareOnWhatsApp = () =>
    openWhatsApp(
      buildOrderShareMessage(order, { name: order.customerName, city: order.city }, formatRupees, statusLabel)
    );

  return (
    <PageShell onShopAgain={onShopAgain}>
      {/* Print-only receipt for "Print receipt" (portalled; hidden on screen). */}
      <OrderReceiptPrint order={order} statusLabel={statusLabel} />
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4">
        <div className="space-y-1.5">
          <p className="text-[11px] font-bold text-slate-400 uppercase tracking-wider">Order</p>
          <h1 className="text-2xl sm:text-3xl font-black text-slate-900 font-heading font-mono tracking-tight break-all">
            {order.orderNumber}
          </h1>
          <div className="flex flex-wrap items-center gap-2">
            <StatusBadge
              tone={expiredUnpaid || isCancelled ? 'rose' : isPendingPayment ? 'amber' : 'emerald'}
              label={statusLabel}
            />
            {isRefunded && <StatusBadge tone="slate" label="Refunded" />}
            {order.isTest && <StatusBadge tone="amber" label="Test order" />}
          </div>
          <p className="text-xs text-slate-500">
            Placed {formatDateTime(order.createdAt) ?? ''} &bull; {order.customerName}
            {order.city ? `, ${order.city}` : ''}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={copyLink}
            className="inline-flex items-center gap-1.5 text-xs font-bold px-4 py-2 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-slate-800 transition-all shadow-2xs"
          >
            {linkCopied ? <Check className="w-4 h-4 text-emerald-600" /> : <Copy className="w-4 h-4 text-slate-600" />}
            <span>{linkCopied ? 'Link copied!' : 'Copy order link'}</span>
          </button>
          <button
            type="button"
            onClick={shareOnWhatsApp}
            className="inline-flex items-center gap-1.5 text-xs font-bold px-4 py-2 rounded-xl bg-[#25D366] hover:bg-[#20ba5a] text-white transition-all shadow-2xs"
          >
            <MessageCircle className="w-4 h-4" />
            <span>{isPaidFlow || isPayLater ? 'Share on WhatsApp' : 'WhatsApp Support'}</span>
          </button>
        </div>
      </div>

      {/* Save-the-link notice */}
      <div className="mt-5 flex items-start gap-2.5 rounded-2xl border border-slate-200 bg-white p-3.5 text-xs text-slate-600">
        <Bookmark className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
        <p>
          <strong className="text-slate-800">Bookmark or save this page link.</strong> It is the only way to view
          this order and contains your private access key — please don&apos;t share it publicly.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 mt-6 items-start">
        {/* Status panel */}
        <div className="lg:col-span-7 space-y-4">
          {showConfirming ? (
            <Panel tone="slate">
              <div className="flex items-start gap-3" role="status" aria-live="polite">
                <Loader2 className="w-6 h-6 animate-spin text-amber-500 shrink-0" />
                <div className="space-y-1">
                  <h2 className="text-base font-black text-slate-900">Confirming your payment…</h2>
                  <p className="text-xs text-slate-500 leading-relaxed">
                    This usually takes a few seconds. Please keep this page open — it updates automatically.
                  </p>
                </div>
              </div>
            </Panel>
          ) : awaitingOnlinePayment && payment.phase === 'confirm_timeout' ? (
            <Panel tone="amber">
              <div className="flex items-start gap-3">
                <Clock className="w-6 h-6 text-amber-600 shrink-0" />
                <div className="space-y-2">
                  <h2 className="text-base font-black text-slate-900">Still confirming your payment</h2>
                  <p className="text-xs text-slate-600 leading-relaxed">
                    We haven&apos;t received confirmation from the payment provider yet. If money was deducted, the
                    order will be confirmed automatically or the amount refunded. You can check again below or come
                    back to this link later.
                  </p>
                  <button
                    type="button"
                    onClick={() => {
                      payment.startConfirming();
                      refresh();
                    }}
                    className="inline-flex items-center gap-2 bg-[#0F172A] hover:bg-slate-800 text-white text-xs font-extrabold px-4 py-2.5 rounded-xl"
                  >
                    <RotateCcw className="w-4 h-4 text-amber-400" />
                    Check again
                  </button>
                </div>
              </div>
            </Panel>
          ) : expiredUnpaid ? (
            <Panel tone="rose">
              <div className="flex items-start gap-3">
                <PackageX className="w-6 h-6 text-rose-600 shrink-0" />
                <div className="space-y-2">
                  <h2 className="text-base font-black text-slate-900">Payment window expired — items released</h2>
                  <p className="text-xs text-slate-600 leading-relaxed">
                    We didn&apos;t receive payment in time, so the reserved items went back into stock. If money was
                    deducted, it will be confirmed automatically or refunded. You can place a fresh order any time.
                  </p>
                  <ShopAgainButton onClick={onShopAgain} />
                </div>
              </div>
            </Panel>
          ) : isCancelled ? (
            <Panel tone="rose">
              <div className="flex items-start gap-3">
                <PackageX className="w-6 h-6 text-rose-600 shrink-0" />
                <div className="space-y-2">
                  <h2 className="text-base font-black text-slate-900">This order was cancelled</h2>
                  <p className="text-xs text-slate-600 leading-relaxed">
                    {isRefunded
                      ? 'Your payment has been refunded to the original payment method.'
                      : 'Please contact us on WhatsApp if you have any questions about this order.'}
                  </p>
                  <ShopAgainButton onClick={onShopAgain} />
                </div>
              </div>
            </Panel>
          ) : awaitingOnlinePayment ? (
            <Panel tone="amber">
              <div className="space-y-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="space-y-1">
                    <h2 className="text-lg font-black text-slate-900">Complete your payment</h2>
                    <p className="text-xs text-slate-600 leading-relaxed">
                      Your items are reserved. Pay securely with UPI, cards or net banking via Razorpay.
                    </p>
                  </div>
                  {msLeft !== null && (
                    <div className="text-right shrink-0" aria-live="off">
                      <span className="text-[10px] font-black uppercase tracking-wider text-amber-700 block">
                        Reserved for
                      </span>
                      <span className="text-xl font-black text-amber-950 font-mono tabular-nums">
                        {formatCountdown(msLeft)}
                      </span>
                    </div>
                  )}
                </div>

                {payment.failureReason && (
                  <div role="alert" className="rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-900 flex gap-2">
                    <AlertCircle className="w-4 h-4 text-rose-600 shrink-0 mt-0.5" />
                    <span>
                      <strong>Payment failed:</strong> {payment.failureReason}. If any amount was deducted for this
                      attempt, your bank will reverse it. You can try again.
                    </span>
                  </div>
                )}

                <button
                  type="button"
                  onClick={() => void payment.pay()}
                  disabled={busy}
                  aria-busy={busy}
                  className="w-full bg-[#0F172A] hover:bg-slate-800 active:scale-98 text-white py-3.5 rounded-2xl text-sm font-extrabold transition-all shadow-md flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {busy ? (
                    <>
                      <Loader2 className="w-4 h-4 animate-spin text-amber-400" />
                      <span>{payment.phase === 'open' ? 'Payment window open…' : 'Opening secure payment…'}</span>
                    </>
                  ) : (
                    <>
                      <CreditCard className="w-4 h-4 text-amber-400" />
                      <span>
                        {payment.failureReason ? 'Try again' : 'Pay'} ₹{formatRupees(order.totalAmount)} now
                      </span>
                    </>
                  )}
                </button>
                <p className="text-[10px] text-slate-500 text-center flex items-center justify-center gap-1">
                  <ShieldCheck className="w-3.5 h-3.5 text-emerald-600" />
                  Payments are processed by Razorpay. We never see your card or UPI PIN.
                </p>
              </div>
            </Panel>
          ) : (
            <Panel tone="emerald">
              <div className="space-y-4">
                <div className="flex items-start gap-3">
                  <div className="w-12 h-12 rounded-2xl bg-emerald-100 text-emerald-600 flex items-center justify-center shrink-0">
                    <Check className="w-6 h-6" />
                  </div>
                  <div className="space-y-1">
                    <h2 className="text-lg font-black text-slate-900">
                      {order.status === 'delivered'
                        ? 'Delivered — enjoy the celebrations!'
                        : isPayLater
                          ? 'Order received! 🎉'
                          : 'Payment received — thank you!'}
                    </h2>
                    <p className="text-xs text-slate-600 leading-relaxed">
                      {isRefunded
                        ? 'This payment has been refunded.'
                        : isPayLater
                          ? 'Our team will contact you on your phone number to confirm payment and dispatch. No online payment is needed now.'
                          : 'Your order is confirmed and is being handled directly at our Sivakasi unit.'}
                    </p>
                  </div>
                </div>

                {isPaidFlow && <StatusSteps status={order.status} />}

                <div className="flex items-start gap-2.5 rounded-2xl border border-slate-200 bg-white p-3 text-xs text-slate-600">
                  <Mail className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
                  <div className="space-y-0.5">
                    <p>
                      We&apos;ve emailed your confirmation and tracking link to{' '}
                      {confirmationEmail ? (
                        <strong className="text-slate-800">{maskEmail(confirmationEmail)}</strong>
                      ) : (
                        'your email address'
                      )}
                      .
                    </p>
                    <p className="text-slate-500">
                      Didn&apos;t get it? Check spam, or bookmark this page — this link is your order tracker.
                    </p>
                  </div>
                </div>

                {isPayLater && (
                  <div className="flex items-start gap-2.5 rounded-2xl border border-emerald-200 bg-white p-3 text-xs text-slate-600">
                    <Clock className="w-4 h-4 text-emerald-600 shrink-0 mt-0.5" />
                    <span>
                      Payment status: <strong className="text-slate-800">to be confirmed with our team</strong>. Keep
                      your order number <strong className="font-mono text-slate-800">{order.orderNumber}</strong> handy
                      when we call.
                    </span>
                  </div>
                )}

                <dl className="grid grid-cols-2 gap-3 text-xs">
                  {instrumentLabel(order.payment.instrument) && (
                    <InfoItem label="Paid via" value={instrumentLabel(order.payment.instrument) as string} />
                  )}
                  {formatDateTime(order.payment.paidAt) && (
                    <InfoItem label="Paid on" value={formatDateTime(order.payment.paidAt) as string} />
                  )}
                  {order.shipping.transportPartner && (
                    <InfoItem label="Transport partner" value={order.shipping.transportPartner} />
                  )}
                  {order.shipping.docketNumber && <InfoItem label="Docket / LR number" value={order.shipping.docketNumber} mono />}
                  {formatDateTime(order.shipping.dispatchedAt) && (
                    <InfoItem label="Dispatched" value={formatDateTime(order.shipping.dispatchedAt) as string} />
                  )}
                  {formatDateTime(order.shipping.deliveredAt) && (
                    <InfoItem label="Delivered" value={formatDateTime(order.shipping.deliveredAt) as string} />
                  )}
                </dl>

                <div className="flex flex-wrap gap-2 pt-1">
                  <button
                    type="button"
                    onClick={() => window.print()}
                    className="inline-flex items-center gap-2 text-xs font-bold px-4 py-2.5 rounded-xl border border-slate-300 bg-white hover:bg-slate-50 text-slate-700"
                  >
                    <Printer className="w-4 h-4" />
                    Print receipt
                  </button>
                  <ShopAgainButton onClick={onShopAgain} label="Continue shopping" />
                </div>
              </div>
            </Panel>
          )}

          {payment.notice && (
            <div
              role="alert"
              className={cn(
                'rounded-2xl border p-3.5 text-xs flex items-start gap-2.5',
                payment.notice.tone === 'error'
                  ? 'border-rose-200 bg-rose-50/70 text-rose-900'
                  : 'border-slate-200 bg-white text-slate-700'
              )}
            >
              <AlertCircle className="w-4 h-4 shrink-0 mt-0.5" />
              <p className="flex-1 font-semibold leading-relaxed">{payment.notice.message}</p>
              <button
                type="button"
                onClick={payment.dismissNotice}
                className="p-0.5 opacity-60 hover:opacity-100"
                aria-label="Dismiss"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          )}
        </div>

        {/* Items & totals */}
        <div className="lg:col-span-5 bg-white border border-slate-200 rounded-3xl p-5 shadow-xs space-y-4">
          <div className="flex items-center justify-between border-b border-slate-100 pb-3">
            <h2 className="text-xs font-black text-slate-800 uppercase tracking-wide flex items-center gap-2">
              <Package className="w-4 h-4 text-amber-600" />
              Items ({order.itemCount} units)
            </h2>
          </div>
          <div className="divide-y divide-slate-100 max-h-80 overflow-y-auto pr-1">
            {order.items.map((item) => (
              <div key={item.productId} className="py-2.5 flex items-center justify-between gap-3 text-xs">
                <div className="min-w-0">
                  <p className="font-bold text-slate-800 truncate">{item.productName}</p>
                  <p className="text-[10px] text-slate-400 font-medium">
                    {item.packInfo ? <>{item.packInfo} &bull; </> : null}
                    {item.quantity} &times; ₹{formatRupees(item.unitPrice)}
                  </p>
                </div>
                <span className="font-bold text-slate-700 shrink-0">₹{formatRupees(item.lineTotal)}</span>
              </div>
            ))}
          </div>
          <div className="border-t border-slate-200 pt-3 space-y-1.5 text-xs">
            {!order.isTest && goodsSavings > 0 && (
              <>
                <div className="flex items-center justify-between text-slate-500">
                  <span>Total MRP</span>
                  <span className="line-through">₹{formatRupees(order.mrpTotal)}</span>
                </div>
                <div className="flex items-center justify-between text-emerald-600 font-bold">
                  <span>You saved</span>
                  <span>₹{formatRupees(goodsSavings)}</span>
                </div>
              </>
            )}
            {order.isTest && (
              <p className="text-[11px] font-semibold text-amber-700">Test order — charged at ₹1 per unit.</p>
            )}
            {!order.isTest && order.pricingTier && (
              <PricingTierNote
                tier={order.pricingTier}
                bulkPercent={pricing.bulkDiscountPercent}
                standardPercent={pricing.standardDiscountPercent}
                bulkMinOrder={pricing.bulkMinOrder}
              />
            )}
            <BillingLines
              subtotal={order.subtotal}
              deliveryFee={order.deliveryFee}
              waivedFee={!order.isTest && order.deliveryFee === 0 && hasDeliveryCharge(pricing) ? pricing.deliveryFee : 0}
              className="pt-1"
            />
            <div className="flex items-center justify-between text-sm font-black text-slate-900 pt-2 border-t border-slate-100">
              <span>
                {isPaid ? 'Amount paid' : isPayLater ? 'Amount payable' : 'Order total'}
                <span className="block text-[10px] font-bold text-emerald-600">{NO_TAX_NOTE}</span>
              </span>
              <span>₹{formatRupees(order.totalAmount)}</span>
            </div>
            {clientEstimate && !order.isTest && Math.abs(clientEstimate.total - order.totalAmount) >= 0.01 && (
              <p className="text-[11px] text-slate-500 leading-relaxed pt-1">
                Prices were re-checked when your order was placed (your cart showed ₹
                {formatRupees(clientEstimate.total)}). The total above is the final amount.
              </p>
            )}
          </div>
          <div className="p-3 bg-amber-50/60 rounded-2xl border border-amber-200/70 text-[11px] text-amber-900 flex items-start gap-2">
            <Truck className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
            <span>Packed in safety cartons and dispatched via insured fireworks transport partners.</span>
          </div>
        </div>
      </div>
    </PageShell>
  );
};

// ---------------------------------------------------------------------------
// Small presentational helpers
// ---------------------------------------------------------------------------

const PageShell: React.FC<{ children: React.ReactNode; onShopAgain: () => void }> = ({ children, onShopAgain }) => (
  <div className="max-w-6xl mx-auto px-4 sm:px-6 py-8 animate-in fade-in duration-200 text-left">
    <div className="pb-6 mb-6 border-b border-slate-200">
      <button
        type="button"
        onClick={onShopAgain}
        className="inline-flex items-center gap-2 text-sm font-bold text-slate-700 hover:text-slate-900 bg-white border border-slate-200 px-4 py-2 rounded-xl transition-all shadow-2xs hover:bg-slate-50 active:scale-97"
      >
        <ArrowLeft className="w-4 h-4" />
        <span>Back to Shop</span>
      </button>
    </div>
    {children}
  </div>
);

const StateCard: React.FC<{ icon: React.ReactNode; title: string; body: string; action?: React.ReactNode }> = ({
  icon,
  title,
  body,
  action,
}) => (
  <div className="py-14 max-w-lg mx-auto text-center space-y-5">
    <div className="w-20 h-20 rounded-3xl bg-slate-100 text-slate-400 flex items-center justify-center mx-auto shadow-inner">
      {icon}
    </div>
    <div className="space-y-2">
      <h1 className="text-2xl font-black text-slate-900 font-heading">{title}</h1>
      <p className="text-sm text-slate-500 leading-relaxed">{body}</p>
    </div>
    {action}
  </div>
);

const ShopAgainButton: React.FC<{ onClick: () => void; label?: string }> = ({ onClick, label = 'Shop again' }) => (
  <button
    type="button"
    onClick={onClick}
    className="inline-flex items-center gap-2 bg-[#F59E0B] hover:bg-[#D97706] text-[#0F172A] font-extrabold text-xs px-5 py-2.5 rounded-xl transition-all shadow-xs active:scale-97"
  >
    <ShoppingBag className="w-4 h-4" />
    <span>{label}</span>
  </button>
);

/** Which price tier the server charged for this order. */
const PricingTierNote: React.FC<{
  tier: PricingTier;
  bulkPercent: number;
  standardPercent: number;
  bulkMinOrder: number;
}> = ({ tier, bulkPercent, standardPercent, bulkMinOrder }) =>
  tier === 'bulk' ? (
    <p className="flex items-center gap-1.5 text-[11px] font-bold text-emerald-700 bg-emerald-50 border border-emerald-200 rounded-xl px-2.5 py-1.5">
      <BadgePercent className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
      <span>
        {bulkPercent}% bulk price applied <span aria-hidden="true">🎉</span>
      </span>
    </p>
  ) : (
    <p className="flex items-start gap-1.5 text-[11px] font-semibold text-amber-800 bg-amber-50 border border-amber-200 rounded-xl px-2.5 py-1.5">
      <BadgePercent className="w-3.5 h-3.5 shrink-0 mt-px" aria-hidden="true" />
      <span>
        {standardPercent}% standard price applied — orders of ₹{formatRupees(bulkMinOrder)}+ get {bulkPercent}% OFF.
      </span>
    </p>
  );

const PANEL_TONES = {
  amber: 'border-amber-200 bg-amber-50/40',
  emerald: 'border-emerald-200 bg-emerald-50/30',
  rose: 'border-rose-200 bg-rose-50/40',
  slate: 'border-slate-200 bg-white',
} as const;

const Panel: React.FC<{ tone: keyof typeof PANEL_TONES; children: React.ReactNode }> = ({ tone, children }) => (
  <section className={cn('rounded-3xl border p-5 sm:p-6 shadow-xs', PANEL_TONES[tone])}>{children}</section>
);

const BADGE_TONES = {
  amber: 'bg-amber-50 text-amber-800 border-amber-200',
  emerald: 'bg-emerald-50 text-emerald-700 border-emerald-200',
  rose: 'bg-rose-50 text-rose-700 border-rose-200',
  slate: 'bg-slate-100 text-slate-700 border-slate-200',
} as const;

const StatusBadge: React.FC<{ tone: keyof typeof BADGE_TONES; label: string }> = ({ tone, label }) => (
  <span
    className={cn(
      'inline-flex items-center text-[11px] font-extrabold uppercase tracking-wide px-2.5 py-1 rounded-full border',
      BADGE_TONES[tone]
    )}
  >
    {label}
  </span>
);

const InfoItem: React.FC<{ label: string; value: string; mono?: boolean }> = ({ label, value, mono }) => (
  <div className="p-3 bg-white border border-slate-200 rounded-xl">
    <dt className="text-[10px] text-slate-400 font-semibold">{label}</dt>
    <dd className={cn('font-bold text-slate-800 break-words', mono && 'font-mono')}>{value}</dd>
  </div>
);

const StatusSteps: React.FC<{ status: string }> = ({ status }) => {
  const current = PAID_FLOW.indexOf(status as (typeof PAID_FLOW)[number]);
  return (
    <ol className="grid grid-cols-5 gap-1.5" aria-label="Order progress">
      {PAID_FLOW.map((step, idx) => {
        const done = idx <= current;
        return (
          <li key={step} className="space-y-1.5" aria-current={idx === current ? 'step' : undefined}>
            <div className={cn('h-1.5 rounded-full', done ? 'bg-emerald-500' : 'bg-slate-200')} />
            <p className={cn('text-[10px] font-bold leading-tight', done ? 'text-emerald-700' : 'text-slate-400')}>
              {STEP_LABELS[step]}
            </p>
          </li>
        );
      })}
    </ol>
  );
};
