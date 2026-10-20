import React, { useState, useEffect } from 'react';
import {
  AlertTriangle,
  ArrowLeft,
  Ban,
  CheckCircle2,
  Clock,
  CreditCard,
  ExternalLink,
  FlaskConical,
  Hourglass,
  Mail,
  MapPin,
  MessageCircle,
  Package,
  Phone,
  Printer,
  RefreshCw,
  ShoppingBag,
  Truck,
  Wallet,
  XCircle,
} from 'lucide-react';
import { useAdminOrder, useOrderLookup } from '../../hooks/admin/adminQueries';
import type { Order, PaymentAction } from '../../types/admin';
import {
  formatCount,
  formatCountdown,
  formatDateTime,
  formatINR,
  nextOrderStatuses,
  ORDER_STATUS_META,
  PAYMENT_STATUS_META,
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
import { useNow } from './useNow';
import { OrderInvoiceModal } from './OrderInvoiceModal';
import { orderCharges } from '../../lib/money';
import {
  AttentionBadge,
  Button,
  ErrorCard,
  OrderStatusBadge,
  PaymentStatusBadge,
  Skeleton,
  Spinner,
  TestBadge,
  PricingTierBadge,
} from './ui';

export type OrderTarget = { id: string; seed?: Order } | { orderNumber: string };

interface Props {
  target: OrderTarget;
  onBack: () => void;
}

interface TimelineEvent {
  at: string;
  label: string;
  detail?: string;
  tone: 'done' | 'bad' | 'neutral';
}

function buildTimeline(o: Order): TimelineEvent[] {
  const ev: TimelineEvent[] = [
    {
      at: o.createdAt,
      label: 'Order placed',
      detail: `${formatCount(o.itemCount)} units · ${formatINR(o.totalAmount)}`,
      tone: 'done',
    },
  ];
  if (o.payment.paidAt)
    ev.push({
      at: o.payment.paidAt,
      label: 'Payment received',
      detail: [paymentMethodSummary(o.payment), o.payment.gatewayPaymentId ?? o.payment.reference].filter(Boolean).join(' · '),
      tone: 'done',
    });
  if (o.shipping.dispatchedAt)
    ev.push({
      at: o.shipping.dispatchedAt,
      label: 'Dispatched',
      detail: [o.shipping.transportPartner, o.shipping.docketNumber].filter(Boolean).join(' · '),
      tone: 'done',
    });
  if (o.shipping.deliveredAt)
    ev.push({
      at: o.shipping.deliveredAt,
      label: 'Delivered',
      tone: 'done',
    });
  if (o.payment.refundedAt)
    ev.push({
      at: o.payment.refundedAt,
      label: 'Payment refunded',
      detail: o.payment.amountRefunded ? formatINR(o.payment.amountRefunded) : undefined,
      tone: 'bad',
    });
  if (o.cancelledAt)
    ev.push({
      at: o.cancelledAt,
      label: 'Cancelled',
      detail: o.cancelReason ?? undefined,
      tone: 'bad',
    });
  return ev.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

function DetailCard({
  title,
  icon: Icon,
  children,
  action,
}: {
  title: string;
  icon?: React.ComponentType<{ className?: string }>;
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <section className="bg-white rounded-3xl border border-slate-200/90 p-5 sm:p-6 shadow-2xs hover:shadow-xs transition-shadow">
      <div className="flex items-center justify-between gap-3 mb-4">
        <h3 className="text-xs font-black uppercase tracking-wider text-slate-500 flex items-center gap-2">
          {Icon && <Icon className="w-4 h-4 text-amber-500" />}
          <span>{title}</span>
        </h3>
        {action}
      </div>
      {children}
    </section>
  );
}

function DataRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-4 text-xs py-1.5 border-b border-slate-50 last:border-none">
      <dt className="text-slate-500 font-bold shrink-0">{label}</dt>
      <dd className="font-extrabold text-slate-900 text-right break-all">{children}</dd>
    </div>
  );
}

export function AdminOrderDetailPage({ target, onBack }: Props) {
  const isIdLookup = 'id' in target;
  const targetId = isIdLookup ? target.id : null;
  const targetNum = !isIdLookup ? target.orderNumber : null;

  const orderQuery = useAdminOrder(targetId);
  const lookupQuery = useOrderLookup(targetNum);

  const activeQuery = isIdLookup ? orderQuery : lookupQuery;
  const order: Order | null | undefined =
    isIdLookup && target.seed && !orderQuery.data ? target.seed : activeQuery.data;

  const [dialog, setDialog] = useState<'status' | 'shipping' | 'cancel' | 'refund' | null>(null);
  const [paymentAction, setPaymentAction] = useState<PaymentAction | null>(null);
  const [showInvoice, setShowInvoice] = useState(false);

  const canAdvance = order ? nextOrderStatuses(order.status).length > 0 : false;
  const awaitingPayment = order?.status === 'pending_payment';
  const canEditShipping = order ? !awaitingPayment && order.status !== 'delivered' && order.status !== 'cancelled' : false;
  const canCancel = order ? order.status === 'pending_pack' || awaitingPayment : false;

  const now = useNow(1000, !!awaitingPayment);
  const manualPayment = order ? availablePaymentActions(order) : [];
  const countdown = order ? formatCountdown(order.paymentExpiresAt, now) : null;
  const wa = order ? whatsappLink(order) : null;
  const timeline = order ? buildTimeline(order) : [];

  // Scroll to top upon load
  useEffect(() => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, []);

  if (activeQuery.isPending && !order) {
    return (
      <div className="space-y-6">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-2 text-xs font-bold text-slate-600 hover:text-slate-900 px-3 py-1.5 rounded-xl hover:bg-slate-100 transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          <span>Back to Orders</span>
        </button>
        <div className="space-y-4">
          <Skeleton className="h-14 rounded-2xl" />
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            <Skeleton className="h-64 rounded-3xl" />
            <Skeleton className="h-64 rounded-3xl" />
          </div>
        </div>
      </div>
    );
  }

  if (activeQuery.isError && !order) {
    return (
      <div className="space-y-6">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-2 text-xs font-bold text-slate-600 hover:text-slate-900 px-3 py-1.5 rounded-xl hover:bg-slate-100 transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          <span>Back to Orders</span>
        </button>
        <ErrorCard
          title="Could not load order details"
          error={activeQuery.error}
          onRetry={() => void activeQuery.refetch()}
          retrying={activeQuery.isFetching}
        />
      </div>
    );
  }

  if (!order) return null;
  const charges = orderCharges(order);
  // Savings are on goods (subtotal) only — the delivery charge is not part of them.
  const discount = Math.max(0, Math.round((order.mrpTotal - charges.subtotal) * 100) / 100);

  return (
    <div className="space-y-6 animate-in fade-in duration-150">
      {/* Top Breadcrumb & Return Bar */}
      <div className="flex flex-wrap items-center justify-between gap-4 pb-2 border-b border-slate-200">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-2 text-xs font-black text-slate-700 hover:text-slate-950 bg-white hover:bg-slate-100 px-3.5 py-2 rounded-xl border border-slate-200 shadow-2xs transition-all cursor-pointer"
        >
          <ArrowLeft className="w-4 h-4 text-amber-500" />
          <span>← Back to Orders & Dispatch</span>
        </button>

        <div className="flex items-center gap-2 text-xs font-bold text-slate-400">
          <span>Orders Console</span>
          <span>/</span>
          <span className="font-mono text-slate-900 font-extrabold">{order.orderNumber}</span>
          {activeQuery.isFetching && <Spinner label="Refreshing…" />}
        </div>
      </div>

      {/* Main Order Header Banner */}
      <div className="bg-white rounded-3xl border border-slate-200 p-6 shadow-2xs flex flex-col lg:flex-row lg:items-center justify-between gap-6">
        <div>
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="font-mono text-2xl sm:text-3xl font-black text-slate-950 tracking-tight">
              {order.orderNumber}
            </h1>
            <OrderStatusBadge status={order.status} />
            <PaymentStatusBadge status={order.payment.status} />
            {order.isTest && <TestBadge />}
            {order.pricingTier && !order.isTest && <PricingTierBadge tier={order.pricingTier} />}
          </div>
          <p className="text-xs font-semibold text-slate-500 mt-1.5 flex flex-wrap items-center gap-2">
            <span>Placed on {formatDateTime(order.createdAt)}</span>
            <span>•</span>
            <span>Customer: <strong className="text-slate-900">{order.customer.name}</strong> ({order.customer.city})</span>
          </p>
        </div>

        {/* Header Action Buttons */}
        <div className="flex flex-wrap items-center gap-2">
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
              className="inline-flex items-center gap-1.5 px-3.5 py-2.5 rounded-xl text-xs font-bold bg-emerald-50 hover:bg-emerald-100 text-emerald-800 border border-emerald-200 shadow-2xs transition-all active:scale-98"
            >
              <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
              <span>WhatsApp update</span>
            </a>
          )}
          {canCancel && (
            <Button variant="dangerOutline" icon={Ban} onClick={() => setDialog('cancel')}>
              Cancel order
            </Button>
          )}
        </div>
      </div>

      {/* Warning/Attention Alerts */}
      {order.payment.attention && (
        <div role="alert" className="p-4 rounded-3xl bg-rose-50 border-2 border-rose-300 text-xs text-rose-900 flex gap-3 shadow-xs">
          <AlertTriangle className="w-5 h-5 shrink-0 mt-0.5 text-rose-600" />
          <div>
            <p className="font-black text-sm">Payment needs attention</p>
            <p className="mt-0.5">{order.payment.attention}</p>
            <p className="mt-1 text-rose-700">Check Payment history and the Cashfree Merchant Dashboard, then refund or contact customer.</p>
          </div>
        </div>
      )}

      {awaitingPayment && countdown && (
        <div
          className={`p-4 rounded-3xl border text-xs flex gap-3 items-center ${
            countdown.urgent ? 'bg-amber-50 border-amber-300 text-amber-900' : 'bg-sky-50 border-sky-200 text-sky-900'
          }`}
        >
          <Hourglass className="w-5 h-5 shrink-0" />
          <div className="flex-1 flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div>
              <p className="font-black text-sm">Awaiting online payment</p>
              <p className="mt-0.5 text-slate-600">Stock remains reserved until the payment window expires.</p>
            </div>
            <span className="font-mono font-black text-base px-3 py-1 bg-white/80 rounded-xl border border-current">
              {countdown.label}
            </span>
          </div>
        </div>
      )}

      {/* Main 2-Column Content Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Left Column: Customer & Shipping */}
        <div className="space-y-6">
          {/* Customer Card */}
          <DetailCard title="Customer details" icon={MapPin}>
            <div className="space-y-1">
              <p className="text-base font-black text-slate-900">{order.customer.name}</p>
              <div className="flex flex-wrap items-center gap-3 pt-1">
                <a
                  href={`tel:${order.customer.phone}`}
                  className="inline-flex items-center gap-1.5 text-xs font-extrabold text-blue-600 hover:text-blue-800"
                >
                  <Phone className="w-3.5 h-3.5" />
                  <span>{order.customer.phone}</span>
                </a>
                {order.customer.email && (
                  <a
                    href={`mailto:${order.customer.email}`}
                    className="inline-flex items-center gap-1.5 text-xs font-semibold text-slate-600 hover:text-slate-900"
                  >
                    <Mail className="w-3.5 h-3.5" />
                    <span>{order.customer.email}</span>
                  </a>
                )}
              </div>
              <p className="text-xs text-slate-600 pt-2 leading-relaxed">
                {order.customer.address}
              </p>
              <p className="text-xs font-bold text-slate-800">
                {order.customer.city}, {order.customer.state} - {order.customer.pincode}
              </p>
            </div>
          </DetailCard>

          {/* Shipping & Transport Card */}
          <DetailCard
            title="Sivakasi Transport & Docket"
            icon={Truck}
            action={
              canEditShipping ? (
                <button
                  type="button"
                  onClick={() => setDialog('shipping')}
                  className="text-xs font-extrabold text-blue-600 hover:text-blue-800 cursor-pointer"
                >
                  Edit Carrier
                </button>
              ) : undefined
            }
          >
            <div className="space-y-1">
              <DataRow label="Transport partner">
                {order.shipping.transportPartner || <span className="text-slate-400 font-semibold">Not assigned</span>}
              </DataRow>
              <DataRow label="Docket / LR number">
                {order.shipping.docketNumber ? (
                  <span className="font-mono text-blue-700 font-black">{order.shipping.docketNumber}</span>
                ) : (
                  <span className="text-slate-400 font-semibold">Not assigned</span>
                )}
              </DataRow>
              {order.shipping.dispatchedAt && (
                <DataRow label="Dispatched at">{formatDateTime(order.shipping.dispatchedAt)}</DataRow>
              )}
              {order.shipping.deliveredAt && (
                <DataRow label="Delivered at">{formatDateTime(order.shipping.deliveredAt)}</DataRow>
              )}
            </div>
          </DetailCard>

          {/* Timeline & Audit Trail */}
          <DetailCard title="Order timeline" icon={Clock}>
            <ol className="relative border-l-2 border-slate-200 ml-2.5 my-2 space-y-4">
              {timeline.map((ev, i) => (
                <li key={i} className="ml-5">
                  <span
                    className={`absolute -left-1.5 mt-1 w-3 h-3 rounded-full border-2 border-white ${
                      ev.tone === 'done' ? 'bg-emerald-500' : ev.tone === 'bad' ? 'bg-rose-500' : 'bg-slate-400'
                    }`}
                  />
                  <div className="flex flex-wrap items-baseline justify-between gap-x-2">
                    <span className="text-xs font-black text-slate-900">{ev.label}</span>
                    <time className="text-[11px] font-semibold text-slate-400 tabular-nums">
                      {formatDateTime(ev.at)}
                    </time>
                  </div>
                  {ev.detail && <p className="text-xs text-slate-600 mt-0.5">{ev.detail}</p>}
                </li>
              ))}
            </ol>
          </DetailCard>
        </div>

        {/* Right Column: Payment Details & Items */}
        <div className="space-y-6">
          {/* Payment Details Card */}
          <DetailCard title="Payment & settlement" icon={CreditCard}>
            <PaymentPanel
              order={order}
              onRefund={() => setDialog('refund')}
              onManualAction={(act) => setPaymentAction(act)}
            />
          </DetailCard>

          {/* Payment History & Events */}
          <DetailCard title="Payment history & gateway events" icon={Wallet}>
            <PaymentHistoryPanel orderId={order.id} />
          </DetailCard>

          {/* Consignment Items Table */}
          <DetailCard title={`Consignment items (${order.items.length})`} icon={ShoppingBag}>
            <div className="divide-y divide-slate-100">
              {order.items.map((it, idx) => (
                <div key={idx} className="py-2.5 flex items-center justify-between text-xs">
                  <div className="pr-3">
                    <p className="font-bold text-slate-900">{it.productName}</p>
                    <p className="text-[11px] text-slate-500">
                      {it.quantity} × {formatINR(it.unitPrice)}
                      {it.unitMrp > it.unitPrice && (
                        <span className="ml-1.5 line-through text-slate-400">
                          {formatINR(it.unitMrp)}
                        </span>
                      )}
                    </p>
                  </div>
                  <span className="font-mono font-black text-slate-950 shrink-0">
                    {formatINR(it.lineTotal)}
                  </span>
                </div>
              ))}
            </div>

            <div className="mt-4 pt-4 border-t border-slate-100 space-y-1.5 text-xs">
              <div className="flex justify-between text-slate-500 font-semibold">
                <span>Total Items</span>
                <span>{order.itemCount} units</span>
              </div>
              {discount > 0 && (
                <div className="flex justify-between text-emerald-700 font-bold">
                  <span>Festival Discount (Savings)</span>
                  <span>−{formatINR(discount)}</span>
                </div>
              )}
              <div className="flex justify-between text-slate-700 font-bold">
                <span>Subtotal</span>
                <span>{formatINR(charges.subtotal)}</span>
              </div>
              <div className="flex justify-between text-slate-500 font-semibold">
                <span>Delivery charge</span>
                {charges.deliveryFee > 0 ? (
                  <span className="text-slate-700">{formatINR(charges.deliveryFee)}</span>
                ) : (
                  <span className="text-emerald-600 font-black">FREE</span>
                )}
              </div>
              <div className="flex justify-between text-slate-500 font-semibold">
                <span>Tax (GST)</span>
                <span className="text-slate-700">{formatINR(0)}</span>
              </div>
              <div className="flex justify-between text-base font-black text-slate-950 pt-2 border-t border-slate-100">
                <span>Total Payable:</span>
                <span className="text-amber-600 font-heading text-lg">
                  {formatINR(order.totalAmount)}
                </span>
              </div>
            </div>
          </DetailCard>
        </div>
      </div>

      {/* Dialogs */}
      {dialog === 'status' && (
        <OrderStatusDialog order={order} open={dialog === 'status'} onClose={() => setDialog(null)} />
      )}
      {dialog === 'shipping' && (
        <OrderShippingDialog order={order} open={dialog === 'shipping'} onClose={() => setDialog(null)} />
      )}
      {dialog === 'cancel' && (
        <OrderCancelDialog order={order} open={dialog === 'cancel'} onClose={() => setDialog(null)} />
      )}
      {dialog === 'refund' && (
        <OrderRefundDialog order={order} open={dialog === 'refund'} onClose={() => setDialog(null)} />
      )}
      {paymentAction && (
        <OrderPaymentDialog
          order={order}
          action={paymentAction}
          onClose={() => setPaymentAction(null)}
        />
      )}
      {showInvoice && (
        <OrderInvoiceModal order={order} onClose={() => setShowInvoice(false)} />
      )}
    </div>
  );
}
