import React, { useState } from 'react';
import {
  AlertTriangle,
  Ban,
  CheckCircle2,
  Circle,
  CreditCard,
  FlaskConical,
  Hourglass,
  Mail,
  MapPin,
  MessageCircle,
  Phone,
  Printer,
  RefreshCw,
  StickyNote,
  Truck,
  XCircle,
} from 'lucide-react';
import { useAdminOrder, useOrderLookup } from '../../hooks/admin/adminQueries';
import type { Order, PaymentAction } from '../../types/admin';
import {
  formatCount,
  formatDateTime,
  formatCountdown,
  formatINR,
  nextOrderStatuses,
  ORDER_STATUS_META,
  paymentMethodSummary,
  whatsappLink,
} from './format';
import {
  availablePaymentActions,
  OrderCancelDialog,
  OrderPaymentDialog,
  OrderRefundDialog,
  OrderShippingDialog,
  OrderStatusDialog,
} from './OrderActionDialogs';
import { PaymentHistoryPanel, PaymentPanel } from './PaymentDetails';
import { orderCharges } from '../../lib/money';
import { useNow } from './useNow';
import { OrderInvoiceModal } from './OrderInvoiceModal';
import {
  AttentionBadge,
  Button,
  Drawer,
  ErrorCard,
  OrderStatusBadge,
  PaymentStatusBadge,
  Skeleton,
  Spinner,
  TestBadge,
  PricingTierBadge,
} from './ui';

export type OrderTarget = { id: string; seed?: Order } | { orderNumber: string };

function Section({ title, icon: Icon, children }: { title: string; icon?: React.ComponentType<{ className?: string }>; children: React.ReactNode }) {
  return (
    <section className="bg-white rounded-2xl border border-slate-200 p-4 sm:p-5">
      <h3 className="text-[11px] font-extrabold uppercase tracking-wider text-slate-400 mb-3 flex items-center gap-1.5">
        {Icon && <Icon className="w-3.5 h-3.5" />}
        {title}
      </h3>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-3 text-xs py-1">
      <dt className="text-slate-500 font-bold">{label}</dt>
      <dd className="font-extrabold text-slate-900 text-right break-all">{children}</dd>
    </div>
  );
}

interface TimelineEvent {
  at: string;
  label: string;
  detail?: string;
  tone: 'done' | 'bad' | 'neutral';
}

function buildTimeline(o: Order): TimelineEvent[] {
  const ev: TimelineEvent[] = [{ at: o.createdAt, label: 'Order placed', detail: `${formatCount(o.itemCount)} units · ${formatINR(o.totalAmount)}`, tone: 'done' }];
  if (o.payment.paidAt)
    ev.push({
      at: o.payment.paidAt,
      label: 'Payment received',
      detail: [paymentMethodSummary(o.payment), o.payment.razorpayPaymentId ?? o.payment.reference].filter(Boolean).join(' · '),
      tone: 'done',
    });
  if (o.shipping.dispatchedAt)
    ev.push({
      at: o.shipping.dispatchedAt,
      label: 'Dispatched',
      detail: [o.shipping.transportPartner, o.shipping.docketNumber].filter(Boolean).join(' · '),
      tone: 'done',
    });
  if (o.shipping.deliveredAt) ev.push({ at: o.shipping.deliveredAt, label: 'Delivered', tone: 'done' });
  if (o.payment.refundedAt)
    ev.push({
      at: o.payment.refundedAt,
      label: 'Payment refunded',
      detail: o.payment.amountRefunded ? formatINR(o.payment.amountRefunded) : undefined,
      tone: 'bad',
    });
  if (o.cancelledAt) ev.push({ at: o.cancelledAt, label: 'Cancelled', detail: o.cancelReason ?? undefined, tone: 'bad' });
  return ev.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

function OrderBody({ order, isFetching, onRefresh }: { order: Order; isFetching: boolean; onRefresh: () => void }) {
  const [dialog, setDialog] = useState<'status' | 'shipping' | 'cancel' | 'refund' | null>(null);
  const [paymentAction, setPaymentAction] = useState<PaymentAction | null>(null);
  const [showInvoice, setShowInvoice] = useState(false);

  const canAdvance = nextOrderStatuses(order.status).length > 0;
  const awaitingPayment = order.status === 'pending_payment';
  const canEditShipping = !awaitingPayment && order.status !== 'delivered' && order.status !== 'cancelled';
  const canCancel = order.status === 'pending_pack' || awaitingPayment;
  const now = useNow(1000, awaitingPayment);
  const manualPayment = availablePaymentActions(order);
  const countdown = formatCountdown(order.paymentExpiresAt, now);
  const wa = whatsappLink(order);
  const c = order.customer;
  const timeline = buildTimeline(order);
  const { subtotal, deliveryFee } = orderCharges(order);
  const discount = Math.max(0, Math.round((order.mrpTotal - subtotal) * 100) / 100);

  return (
    <div className="p-4 sm:p-6 space-y-4">
      {/* Actions */}
      <div className="bg-white rounded-2xl border border-slate-200 p-3 flex flex-wrap gap-2">
        {manualPayment.includes('mark_paid') && (
          <Button variant="success" icon={CreditCard} onClick={() => setPaymentAction('mark_paid')}>
            Record payment
          </Button>
        )}
        {canAdvance && (
          <Button variant="primary" icon={Truck} onClick={() => setDialog('status')}>
            {order.status === 'pending_pack' ? 'Dispatch / update status' : 'Update status'}
          </Button>
        )}
        {canEditShipping && (
          <Button variant="secondary" icon={Truck} onClick={() => setDialog('shipping')}>
            Transport & docket
          </Button>
        )}
        <Button variant="secondary" icon={Printer} onClick={() => setShowInvoice(true)}>
          Print slip
        </Button>
        {wa && (
          <a
            href={wa}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-bold bg-emerald-50 hover:bg-emerald-100 text-emerald-800 border border-emerald-200"
          >
            <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
            WhatsApp update
          </a>
        )}
        {canCancel && (
          <Button variant="dangerOutline" icon={Ban} onClick={() => setDialog('cancel')} className="sm:ml-auto">
            Cancel order
          </Button>
        )}
      </div>

      {order.payment.attention && (
        <div role="alert" className="p-3.5 rounded-2xl bg-rose-50 border-2 border-rose-300 text-xs text-rose-900 flex gap-2.5">
          <AlertTriangle className="w-4 h-4 shrink-0 mt-px text-rose-600" />
          <div>
            <p className="font-black">Payment needs attention</p>
            <p className="mt-0.5">{order.payment.attention}</p>
            <p className="mt-1 text-rose-700">Check Payment history and the Razorpay dashboard, then refund or contact the customer.</p>
          </div>
        </div>
      )}

      {order.isTest && (
        <div className="p-3 rounded-2xl bg-violet-50 border border-violet-200 text-xs text-violet-900 flex gap-2 items-start">
          <FlaskConical className="w-4 h-4 shrink-0 mt-px" />
          <span>
            <span className="font-black">Test order</span> — placed in payment test mode (₹1 per item). Excluded from revenue; do
            not dispatch real goods unless intended.
          </span>
        </div>
      )}

      {awaitingPayment && (
        <div
          className={`p-3.5 rounded-2xl border text-xs flex gap-2.5 items-start ${
            countdown.urgent ? 'bg-amber-50 border-amber-300 text-amber-900' : 'bg-sky-50 border-sky-200 text-sky-900'
          }`}
        >
          <Hourglass className="w-4 h-4 shrink-0 mt-px" />
          <div>
            <p className="font-black">
              Awaiting online payment · <span className="tabular-nums">{countdown.label}</span>
            </p>
            <p className="mt-0.5">
              Stock is reserved until {formatDateTime(order.paymentExpiresAt)}. The order moves to Pending Pack automatically
              once Razorpay confirms payment; if the window closes it is cancelled and stock released. It cannot be dispatched
              yet, but you can cancel it to release stock now.
            </p>
          </div>
        </div>
      )}

      {order.status === 'cancelled' && (
        <div className="p-3.5 rounded-2xl bg-slate-100 border border-slate-300 text-xs text-slate-700">
          <span className="font-extrabold">Cancelled {formatDateTime(order.cancelledAt)}.</span>{' '}
          {order.cancelReason && <>Reason: {order.cancelReason}. </>}Stock was returned to inventory.
        </div>
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <Section title="Customer" icon={MapPin}>
          <p className="text-sm font-black text-slate-950">{c.name}</p>
          <a href={`tel:${c.phone}`} className="mt-1 text-xs font-bold text-blue-700 hover:underline inline-flex items-center gap-1">
            <Phone className="w-3 h-3" /> {c.phone}
          </a>
          {c.email && (
            <a href={`mailto:${c.email}`} className="block mt-0.5 text-xs font-semibold text-blue-700 hover:underline break-all">
              <Mail className="w-3 h-3 inline mr-1" />
              {c.email}
            </a>
          )}
          <p className="text-xs text-slate-600 mt-2 leading-relaxed whitespace-pre-line">{c.address}</p>
          <p className="text-xs font-extrabold text-slate-900 mt-0.5">
            {c.city}, {c.state} – {c.pincode}
          </p>
        </Section>

        <Section title="Shipping" icon={Truck}>
          <dl>
            <Row label="Transport">{order.shipping.transportPartner ?? <span className="text-slate-400">Not assigned</span>}</Row>
            <Row label="Docket / LR">
              {order.shipping.docketNumber ? (
                <span className="font-mono">{order.shipping.docketNumber}</span>
              ) : (
                <span className="text-slate-400">Not assigned</span>
              )}
            </Row>
            {order.shipping.dispatchedAt && <Row label="Dispatched">{formatDateTime(order.shipping.dispatchedAt)}</Row>}
            {order.shipping.deliveredAt && <Row label="Delivered">{formatDateTime(order.shipping.deliveredAt)}</Row>}
          </dl>
        </Section>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 items-start">
        <PaymentPanel order={order} onRefund={() => setDialog('refund')} onManualAction={setPaymentAction} />
        <PaymentHistoryPanel orderId={order.id} />
      </div>

      {order.notes && (
        <Section title="Customer notes" icon={StickyNote}>
          <p className="text-xs text-slate-700 whitespace-pre-line leading-relaxed">{order.notes}</p>
        </Section>
      )}

      <Section title={`Items (${order.items.length} products · ${formatCount(order.itemCount)} units)`}>
        <div className="overflow-x-auto -mx-1">
          <table className="w-full text-xs">
            <thead>
              <tr className="text-[10px] uppercase tracking-wider text-slate-400 text-left">
                <th className="py-1.5 px-1 font-extrabold">Product</th>
                <th className="py-1.5 px-1 font-extrabold text-right">Qty</th>
                <th className="py-1.5 px-1 font-extrabold text-right">Rate</th>
                <th className="py-1.5 px-1 font-extrabold text-right">Amount</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {order.items.map((it) => (
                <tr key={it.productId}>
                  <td className="py-2 px-1">
                    <p className="font-extrabold text-slate-900">{it.productName}</p>
                    <p className="text-[10px] text-slate-400">
                      {it.packInfo} · <span className="font-mono">{it.productId}</span>
                    </p>
                  </td>
                  <td className="py-2 px-1 text-right font-bold tabular-nums">{formatCount(it.quantity)}</td>
                  <td className="py-2 px-1 text-right tabular-nums">
                    <span className="block">{formatINR(it.unitPrice)}</span>
                    {it.unitMrp > it.unitPrice && (
                      <span className="block text-[10px] text-slate-400 line-through">{formatINR(it.unitMrp)}</span>
                    )}
                  </td>
                  <td className="py-2 px-1 text-right font-black tabular-nums">{formatINR(it.lineTotal)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <dl className="mt-3 pt-3 border-t border-slate-100">
          <Row label="MRP total">{formatINR(order.mrpTotal)}</Row>
          {discount > 0 && <Row label="Discount">−{formatINR(discount)}</Row>}
          <Row label="Subtotal">{formatINR(subtotal)}</Row>
          <Row label="Delivery charge">
            {deliveryFee > 0 ? formatINR(deliveryFee) : <span className="text-emerald-600">FREE</span>}
          </Row>
          <Row label="Tax (GST)">{formatINR(0)}</Row>
          <div className="flex justify-between gap-3 pt-1">
            <dt className="text-sm font-black text-slate-950">Total</dt>
            <dd className="text-base font-black text-amber-600 tabular-nums">{formatINR(order.totalAmount)}</dd>
          </div>
        </dl>
      </Section>

      <Section title="Timeline">
        <ol className="relative border-l-2 border-slate-100 ml-2 space-y-4">
          {timeline.map((e, i) => (
            <li key={i} className="ml-4">
              <span className="absolute -left-[9px] mt-0.5 bg-white">
                {e.tone === 'bad' ? (
                  <XCircle className="w-4 h-4 text-rose-500" />
                ) : (
                  <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                )}
              </span>
              <p className="text-xs font-extrabold text-slate-900">{e.label}</p>
              <p className="text-[11px] text-slate-500">{formatDateTime(e.at)}</p>
              {e.detail && <p className="text-[11px] text-slate-600 mt-0.5">{e.detail}</p>}
            </li>
          ))}
          {(order.status === 'in_transit' || order.status === 'out_for_delivery') && (
            <li className="ml-4">
              <span className="absolute -left-[9px] mt-0.5 bg-white">
                <Circle className="w-4 h-4 text-blue-500" />
              </span>
              <p className="text-xs font-extrabold text-slate-900">Now: {ORDER_STATUS_META[order.status].label}</p>
              <p className="text-[11px] text-slate-500">Last updated {formatDateTime(order.updatedAt)}</p>
            </li>
          )}
        </ol>
      </Section>

      <div className="flex items-center justify-between text-[11px] text-slate-400 px-1">
        <span>
          Last updated {formatDateTime(order.updatedAt)} · v{order.version}
        </span>
        <button type="button" onClick={onRefresh} className="inline-flex items-center gap-1 font-bold hover:text-slate-700" disabled={isFetching}>
          <RefreshCw className={`w-3 h-3 ${isFetching ? 'animate-spin' : ''}`} /> Refresh
        </button>
      </div>

      <OrderStatusDialog order={order} open={dialog === 'status'} onClose={() => setDialog(null)} />
      <OrderShippingDialog order={order} open={dialog === 'shipping'} onClose={() => setDialog(null)} />
      <OrderCancelDialog order={order} open={dialog === 'cancel'} onClose={() => setDialog(null)} />
      <OrderRefundDialog order={order} open={dialog === 'refund'} onClose={() => setDialog(null)} />
      <OrderPaymentDialog order={order} action={paymentAction} onClose={() => setPaymentAction(null)} />
      <OrderInvoiceModal order={showInvoice ? order : null} onClose={() => setShowInvoice(false)} />
    </div>
  );
}

export function OrderDetailDrawer({ target, onClose }: { target: OrderTarget | null; onClose: () => void }) {
  const byNumber = target && 'orderNumber' in target ? target.orderNumber : null;
  const lookup = useOrderLookup(byNumber);
  const id = target ? ('id' in target ? target.id : lookup.data?.id ?? null) : null;
  const seed = target && 'id' in target ? target.seed : lookup.data ?? undefined;
  const query = useAdminOrder(id, seed);
  const order = query.data;

  const title = (
    <div className="flex flex-wrap items-center gap-2">
      <h2 className="font-mono text-lg font-black text-slate-950">
        {order?.orderNumber ?? byNumber ?? 'Order'}
      </h2>
      {order && <OrderStatusBadge status={order.status} />}
      {order && <PaymentStatusBadge status={order.payment.status} />}
      {order?.isTest && <TestBadge />}
      {order?.pricingTier && !order.isTest && <PricingTierBadge tier={order.pricingTier} />}
      {order?.payment.attention && <AttentionBadge title={order.payment.attention} />}
      {query.isFetching && order && <Spinner className="w-3.5 h-3.5" />}
    </div>
  );
  const subtitle = order ? (
    <p className="text-xs text-slate-500 mt-0.5">
      Placed {formatDateTime(order.createdAt)} · {order.customer.name}, {order.customer.city}
    </p>
  ) : null;

  let body: React.ReactNode;
  if (byNumber && lookup.isPending) {
    body = <DrawerSkeleton />;
  } else if (byNumber && lookup.isError) {
    body = (
      <div className="p-6">
        <ErrorCard error={lookup.error} onRetry={() => void lookup.refetch()} compact />
      </div>
    );
  } else if (byNumber && !lookup.data) {
    body = <p className="p-6 text-xs text-slate-500">Order {byNumber} was not found.</p>;
  } else if (!order && query.isPending) {
    body = <DrawerSkeleton />;
  } else if (!order) {
    body = (
      <div className="p-6">
        <ErrorCard title="Could not load this order" error={query.error} onRetry={() => void query.refetch()} compact />
      </div>
    );
  } else {
    body = (
      <>
        {query.isError && (
          <div className="mx-4 sm:mx-6 mt-4 p-3 rounded-xl bg-amber-50 border border-amber-200 text-xs font-bold text-amber-800">
            Showing the last loaded copy — refresh failed.
          </div>
        )}
        <OrderBody order={order} isFetching={query.isFetching} onRefresh={() => void query.refetch()} />
      </>
    );
  }

  return (
    <Drawer open={!!target} onClose={onClose} title={title} subtitle={subtitle} label="Order details">
      {body}
    </Drawer>
  );
}

function DrawerSkeleton() {
  return (
    <div className="p-6 space-y-4" aria-busy="true">
      <Skeleton className="h-14" />
      <div className="grid grid-cols-2 gap-4">
        <Skeleton className="h-36" />
        <Skeleton className="h-36" />
      </div>
      <Skeleton className="h-48" />
    </div>
  );
}
