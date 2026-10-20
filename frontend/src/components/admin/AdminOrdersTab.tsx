import React, { useMemo, useState } from 'react';
import {
  CheckCircle2,
  ChevronRight,
  Clock,
  CreditCard,
  Edit3,
  LayoutGrid,
  List,
  MessageCircle,
  Package,
  Printer,
  Search,
  Truck,
  Wallet,
  X,
} from 'lucide-react';
import { useAdminOrders, useDashboard } from '../../hooks/admin/adminQueries';
import { useDebouncedValue } from '../../hooks/admin/useDebouncedValue';
import type { Order, OrderStatus, PaymentStatus } from '../../types/admin';
import { ORDER_STATUSES, PAYMENT_STATUSES } from '../../types/admin';
import {
  formatCount,
  formatCountdown,
  formatDate,
  formatDateTime,
  formatINR,
  formatTime,
  itemsSummary,
  ORDER_STATUS_META,
  PAYMENT_STATUS_META,
  paymentMethodSummary,
  whatsappLink,
} from './format';
import {
  availablePaymentActions,
  OrderPaymentDialog,
  OrderShippingDialog,
  OrderStatusDialog,
} from './OrderActionDialogs';
import { OrderInvoiceModal } from './OrderInvoiceModal';
import {
  Button,
  EmptyState,
  ErrorCard,
  FilterChip,
  inputClass,
  OrderStatusBadge,
  PaymentStatusBadge,
  Skeleton,
  Spinner,
  AttentionBadge,
  TestBadge,
} from './ui';
import { useNow } from './useNow';
import { useInfiniteSentinel } from './useInfiniteSentinel';

interface Props {
  status: OrderStatus | null;
  onStatusChange: (status: OrderStatus | null) => void;
  highlighted: ReadonlySet<string>;
  onOpenOrder: (order: Order) => void;
}

export function OrderRowsSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="space-y-4" aria-busy="true" aria-label="Loading orders">
      {Array.from({ length: rows }).map((_, i) => (
        <Skeleton key={i} className="h-64 rounded-3xl" />
      ))}
    </div>
  );
}

/** Rich Card view with 4 full columns: Customer, Items, Sivakasi Transport, and Payment details. */
function OrderCard({
  order,
  highlighted,
  onOpen,
  onPrintInvoice,
  onEditShipping,
  onEditStatus,
  onRecordPayment,
  now,
}: {
  order: Order;
  highlighted: boolean;
  onOpen: () => void;
  onPrintInvoice: () => void;
  onEditShipping: () => void;
  onEditStatus: () => void;
  onRecordPayment: () => void;
  now: number;
}) {
  const countdown = order.status === 'pending_payment' ? formatCountdown(order.paymentExpiresAt, now) : null;
  const waLink = whatsappLink(order);
  const canRecordPayment = availablePaymentActions(order).includes('mark_paid');

  return (
    <div
      className={`bg-white rounded-3xl border transition-all p-5 sm:p-6 shadow-2xs hover:shadow-sm ${
        highlighted
          ? 'border-amber-400 ring-2 ring-amber-300/40 bg-amber-50/20'
          : 'border-slate-200 hover:border-slate-300'
      }`}
    >
      {/* Top Bar of Order Card */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-4 border-b border-slate-100">
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="button"
            onClick={onPrintInvoice}
            className="text-base font-black text-slate-950 font-mono tracking-tight hover:text-amber-600 transition-colors flex items-center gap-1.5 cursor-pointer group"
            title="View official order estimation & print PDF"
          >
            <span>{order.orderNumber}</span>
            <Printer className="w-3.5 h-3.5 text-slate-400 group-hover:text-amber-500 transition-colors" />
          </button>
          <span className="text-xs font-bold text-slate-400">
            {formatDate(order.createdAt)} · {formatTime(order.createdAt)}
          </span>
          <span className="text-[11px] font-bold bg-slate-100 text-slate-700 px-2.5 py-1 rounded-lg">
            {paymentMethodSummary(order.payment)}
          </span>
          {order.status !== 'pending_payment' && <PaymentStatusBadge status={order.payment.status} />}
          {order.isTest && <TestBadge />}
          {order.payment.attention && <AttentionBadge title={order.payment.attention} />}
          {highlighted && (
            <span className="text-[9px] font-black uppercase bg-amber-400 text-slate-950 px-1.5 py-0.5 rounded shadow-2xs">
              New
            </span>
          )}
          {countdown && (
            <span
              className={`text-[11px] font-extrabold tabular-nums px-2 py-0.5 rounded-lg border ${
                countdown.urgent
                  ? 'bg-amber-50 text-amber-800 border-amber-200'
                  : 'bg-sky-50 text-sky-800 border-sky-200'
              }`}
            >
              {countdown.label}
            </span>
          )}
        </div>

        {/* Current Status Pill */}
        <div className="flex items-center gap-2">
          <OrderStatusBadge status={order.status} />
        </div>
      </div>

      {/* Middle Details: 4 Columns (Customer, Items, Transport, Payment) */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5 py-4 border-b border-slate-100">
        {/* Column 1: Customer Information */}
        <div>
          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1.5">
            Customer Details
          </p>
          <p className="text-sm font-black text-slate-900">{order.customer.name}</p>
          <p className="text-xs font-semibold text-slate-600 mt-0.5">{order.customer.phone}</p>
          <p className="text-xs text-slate-500 mt-1 leading-relaxed">
            {order.customer.address}, {order.customer.city}
            {order.customer.state ? `, ${order.customer.state}` : ''}
            {order.customer.pincode ? ` - ${order.customer.pincode}` : ''}
          </p>
        </div>

        {/* Column 2: Itemized Order Summary */}
        <div>
          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1.5">
            Consignment Items ({order.items.length})
          </p>
          <div className="space-y-1.5 max-h-36 overflow-y-auto pr-1">
            {order.items.map((it, idx) => (
              <div key={idx} className="flex justify-between text-xs font-semibold">
                <span className="text-slate-800 truncate mr-2">
                  {it.quantity}x {it.productName}
                </span>
                <span className="text-slate-950 font-bold shrink-0">
                  {formatINR(it.lineTotal)}
                </span>
              </div>
            ))}
          </div>
          <div className="mt-2 pt-2 border-t border-slate-100 flex justify-between text-xs font-black">
            <span>Total Payable:</span>
            <span className="text-amber-600 font-heading text-sm">
              {formatINR(order.totalAmount)}
            </span>
          </div>
        </div>

        {/* Column 3: Transport Logistics & Docket */}
        <div>
          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1.5">
            Sivakasi Transport & Docket
          </p>
          <div className="p-3 bg-slate-50 rounded-2xl border border-slate-200/80 space-y-1.5">
            <div className="flex justify-between text-xs">
              <span className="text-slate-500 font-bold">Carrier:</span>
              <span className="font-extrabold text-slate-900">
                {order.shipping.transportPartner || 'VRL Logistics (Default)'}
              </span>
            </div>
            <div className="flex justify-between text-xs">
              <span className="text-slate-500 font-bold">Docket / LR:</span>
              <span className="font-mono font-black text-blue-700">
                {order.shipping.docketNumber || 'Unassigned'}
              </span>
            </div>
            {order.shipping.dispatchedAt && (
              <p className="text-[10px] font-semibold text-slate-500 pt-0.5">
                {formatDateTime(order.shipping.dispatchedAt)}
              </p>
            )}
          </div>

          <button
            type="button"
            onClick={onEditShipping}
            className="mt-2.5 text-xs font-bold text-blue-600 hover:text-blue-800 flex items-center gap-1.5 cursor-pointer transition-colors"
          >
            <Edit3 className="w-3.5 h-3.5" />
            <span>Edit Carrier / Docket No.</span>
          </button>
        </div>

        {/* Column 4: Payment & Settlement Details */}
        <div>
          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1.5">
            Payment & Settlement
          </p>
          <div className="p-3 bg-slate-50 rounded-2xl border border-slate-200/80 space-y-1.5">
            <div className="flex justify-between items-center text-xs">
              <span className="text-slate-500 font-bold">Status:</span>
              <PaymentStatusBadge status={order.payment.status} />
            </div>
            <div className="flex justify-between text-xs">
              <span className="text-slate-500 font-bold">Method:</span>
              <span className="font-extrabold text-slate-900 truncate ml-2">
                {paymentMethodSummary(order.payment)}
              </span>
            </div>
            <div className="flex justify-between text-xs">
              <span className="text-slate-500 font-bold">Payable:</span>
              <span className="font-mono font-black text-slate-900">
                {formatINR(order.totalAmount)}
              </span>
            </div>
            <div className="flex justify-between text-xs">
              <span className="text-slate-500 font-bold">Ref / UTR:</span>
              <span className="font-mono font-bold text-slate-700 truncate ml-2 max-w-[120px]">
                {order.payment.gatewayPaymentId || order.payment.reference || (
                  <span className="text-slate-400 italic">Pending Entry</span>
                )}
              </span>
            </div>
            {order.payment.paidAt && (
              <p className="text-[10px] font-semibold text-emerald-700 pt-0.5">
                Paid: {formatDateTime(order.payment.paidAt)}
              </p>
            )}
          </div>

          {canRecordPayment ? (
            <button
              type="button"
              onClick={onRecordPayment}
              className="mt-2.5 text-xs font-bold text-emerald-700 hover:text-emerald-900 flex items-center gap-1.5 cursor-pointer transition-colors"
            >
              <CreditCard className="w-3.5 h-3.5" />
              <span>Record Payment (Mark Paid)</span>
            </button>
          ) : order.payment.status === 'paid' ? (
            <p className="mt-2.5 text-[11px] font-extrabold text-emerald-600 flex items-center gap-1">
              <CheckCircle2 className="w-3.5 h-3.5" />
              <span>Settlement Complete</span>
            </p>
          ) : (
            <span className="mt-2.5 block text-[11px] font-semibold text-slate-400">
              Awaiting Gateway / Settlement
            </span>
          )}
        </div>
      </div>

      {/* Bottom Status Providings Control Bar */}
      <div className="pt-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="text-xs font-bold text-slate-500">Provide Status:</span>
          <button
            type="button"
            onClick={onEditStatus}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-extrabold border bg-white border-slate-300 text-slate-900 hover:border-amber-500 cursor-pointer shadow-2xs hover:bg-slate-50 transition-all"
          >
            <span>Change Status ({ORDER_STATUS_META[order.status].label})</span>
            <ChevronRight className="w-3.5 h-3.5 text-slate-400" />
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {/* Estimation & Print PDF Button */}
          <button
            type="button"
            onClick={onPrintInvoice}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-black bg-slate-950 hover:bg-slate-900 text-white shadow-xs transition-all active:scale-98 cursor-pointer"
            title="View official Sivakasi order estimation & print PDF"
          >
            <Printer className="w-3.5 h-3.5 text-amber-400" />
            <span>Estimation & Print PDF</span>
          </button>

          {/* Send WhatsApp LR Update */}
          {waLink && (
            <a
              href={waLink}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-bold bg-emerald-50 hover:bg-emerald-100 text-emerald-800 border border-emerald-200 transition-all active:scale-98 cursor-pointer"
            >
              <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
              <span>Send WhatsApp LR Update</span>
            </a>
          )}

          {/* Full Details Redirect Button */}
          <button
            type="button"
            onClick={onOpen}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-bold bg-slate-100 text-slate-800 hover:text-slate-950 hover:bg-slate-200 border border-slate-200 transition-all cursor-pointer shadow-2xs active:scale-98"
            title="Redirect to dedicated order details page"
          >
            <span>Full Details</span>
            <ChevronRight className="w-3.5 h-3.5 text-slate-500" />
          </button>
        </div>
      </div>
    </div>
  );
}

/** Compact single-line row for dense listing mode. */
function OrderRow({
  order,
  highlighted,
  onOpen,
  now,
}: {
  order: Order;
  highlighted: boolean;
  onOpen: () => void;
  now: number;
}) {
  const countdown = order.status === 'pending_payment' ? formatCountdown(order.paymentExpiresAt, now) : null;
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className={`w-full text-left grid grid-cols-2 md:grid-cols-[150px_minmax(0,1.3fr)_minmax(0,1.6fr)_110px_minmax(0,auto)_20px] gap-x-4 gap-y-1.5 items-center px-4 py-3 rounded-2xl border transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500 ${
          highlighted
            ? 'bg-amber-50 border-amber-300 shadow-xs'
            : 'bg-white border-slate-200 hover:border-amber-300 hover:shadow-xs'
        }`}
      >
        <span className="min-w-0">
          <span className="block font-mono text-sm font-black text-slate-950 truncate">
            {order.orderNumber}
            {highlighted && (
              <span className="ml-1.5 align-middle text-[9px] font-black uppercase bg-amber-400 text-slate-950 px-1.5 py-0.5 rounded">
                New
              </span>
            )}
          </span>
          <span className="block text-[11px] text-slate-500 tabular-nums">
            {formatDate(order.createdAt)} · {formatTime(order.createdAt)}
          </span>
        </span>
        <span className="min-w-0 text-right md:text-left">
          <span className="block text-xs font-extrabold text-slate-900 truncate">{order.customer.name}</span>
          <span className="block text-[11px] text-slate-500 truncate">{order.customer.city}</span>
        </span>
        <span className="col-span-2 md:col-span-1 text-[11px] text-slate-600 truncate">
          {itemsSummary(order)} <span className="text-slate-400">· {formatCount(order.itemCount)} units</span>
        </span>
        <span className="text-sm font-black text-slate-950 tabular-nums md:text-right">{formatINR(order.totalAmount)}</span>
        <span className="flex flex-wrap items-center gap-1.5 justify-end md:justify-start">
          <OrderStatusBadge status={order.status} />
          {order.status !== 'pending_payment' && <PaymentStatusBadge status={order.payment.status} />}
          {order.isTest && <TestBadge />}
          {order.payment.attention && <AttentionBadge title={order.payment.attention} />}
          {countdown && (
            <span className={`text-[11px] font-extrabold tabular-nums ${countdown.urgent ? 'text-amber-700' : 'text-sky-700'}`}>
              {countdown.label}
            </span>
          )}
        </span>
        <ChevronRight className="hidden md:block w-4 h-4 text-slate-300" aria-hidden />
      </button>
    </li>
  );
}

export function AdminOrdersTab({ status, onStatusChange, highlighted, onOpenOrder }: Props) {
  const [paymentStatus, setPaymentStatus] = useState<PaymentStatus | null>(null);
  const [search, setSearch] = useState('');
  const [viewMode, setViewMode] = useState<'cards' | 'compact'>('cards');
  const [invoiceOrder, setInvoiceOrder] = useState<Order | null>(null);
  const [shippingOrder, setShippingOrder] = useState<Order | null>(null);
  const [statusOrder, setStatusOrder] = useState<Order | null>(null);
  const [paymentActionOrder, setPaymentActionOrder] = useState<Order | null>(null);

  const q = useDebouncedValue(search.trim(), 350);
  const dashboard = useDashboard();
  const byStatus = dashboard.data?.orders.byStatus;

  const orders = useAdminOrders({ status, paymentStatus, q });
  const rows = useMemo(() => orders.data?.pages.flatMap((p) => p.orders) ?? [], [orders.data]);
  const hasCountdown = rows.some((o) => o.status === 'pending_payment');
  const now = useNow(1000, hasCountdown);
  // "All" excludes orders still awaiting payment (the server hides them unless asked).
  const allCount = byStatus
    ? ORDER_STATUSES.filter((s) => s !== 'pending_payment').reduce((a, s) => a + (byStatus[s] ?? 0), 0)
    : undefined;
  const sentinel = useInfiniteSentinel(
    !!orders.hasNextPage && !orders.isFetchingNextPage && !orders.isFetchNextPageError,
    () => {
      if (orders.hasNextPage && !orders.isFetchingNextPage) void orders.fetchNextPage();
    }
  );
  const filtersActive = !!status || !!paymentStatus || !!q;

  return (
    <div className="space-y-5">
      {/* Header + Search & Controls */}
      <div className="flex flex-col lg:flex-row lg:items-end justify-between gap-4">
        <div>
          <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
            Orders & Sivakasi Dispatch Tracking
          </h2>
          <p className="text-xs text-slate-500 font-semibold mt-0.5">
            Update order fulfillment status, assign transport carriers (VRL/ABT/ARC), and docket numbers.
          </p>
        </div>
        <div className="flex flex-col sm:flex-row items-center gap-2 w-full lg:w-auto">
          {/* Search Box */}
          <div className="relative w-full sm:w-72">
            <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" aria-hidden />
            <label htmlFor="orders-search" className="sr-only">
              Search orders
            </label>
            <input
              id="orders-search"
              type="search"
              maxLength={100}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search by Order ID, name, or phone..."
              className={`${inputClass} pl-10`}
            />
          </div>

          {/* Payment filter dropdown */}
          <label htmlFor="orders-payment" className="sr-only">
            Payment status
          </label>
          <select
            id="orders-payment"
            value={paymentStatus ?? ''}
            onChange={(e) => setPaymentStatus((e.target.value || null) as PaymentStatus | null)}
            className={`${inputClass} w-full sm:w-44`}
          >
            <option value="">All payments</option>
            {PAYMENT_STATUSES.map((p) => (
              <option key={p} value={p}>
                {PAYMENT_STATUS_META[p].label}
              </option>
            ))}
          </select>

          {/* View mode toggle */}
          <div className="flex items-center bg-slate-100 p-1 rounded-xl border border-slate-200 self-end sm:self-auto">
            <button
              type="button"
              onClick={() => setViewMode('cards')}
              title="Card View (GitHub style)"
              className={`p-1.5 rounded-lg transition-colors cursor-pointer ${
                viewMode === 'cards' ? 'bg-white text-slate-950 shadow-2xs font-bold' : 'text-slate-500 hover:text-slate-900'
              }`}
            >
              <LayoutGrid className="w-4 h-4" />
            </button>
            <button
              type="button"
              onClick={() => setViewMode('compact')}
              title="Compact List View"
              className={`p-1.5 rounded-lg transition-colors cursor-pointer ${
                viewMode === 'compact' ? 'bg-white text-slate-950 shadow-2xs font-bold' : 'text-slate-500 hover:text-slate-900'
              }`}
            >
              <List className="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>

      {/* Filter Chips */}
      <div className="flex flex-wrap gap-2 pb-1" role="group" aria-label="Filter by order status">
        <FilterChip active={status === null} onClick={() => onStatusChange(null)} count={allCount}>
          All Orders
        </FilterChip>
        {ORDER_STATUSES.map((s) => (
          <FilterChip key={s} active={status === s} onClick={() => onStatusChange(s)} count={byStatus ? byStatus[s] ?? 0 : undefined}>
            {ORDER_STATUS_META[s].label}
          </FilterChip>
        ))}
      </div>

      {/* Main Content Area */}
      {orders.isPending ? (
        <OrderRowsSkeleton />
      ) : orders.isError && rows.length === 0 ? (
        <ErrorCard
          title="Could not load orders"
          error={orders.error}
          onRetry={() => void orders.refetch()}
          retrying={orders.isFetching}
        />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={Package}
          title={filtersActive ? 'No orders match these filters' : 'No orders yet'}
          description={filtersActive ? 'Try a different status, payment filter or search.' : 'Storefront orders will appear here.'}
          action={
            filtersActive ? (
              <Button
                icon={X}
                onClick={() => {
                  onStatusChange(null);
                  setPaymentStatus(null);
                  setSearch('');
                }}
              >
                Clear filters
              </Button>
            ) : undefined
          }
        />
      ) : viewMode === 'cards' ? (
        // GitHub Rich Card View (Default)
        <div className={`space-y-4 transition-opacity ${orders.isPlaceholderData ? 'opacity-60' : ''}`}>
          {rows.map((o) => (
            <OrderCard
              key={o.id}
              order={o}
              highlighted={highlighted.has(o.orderNumber)}
              onOpen={() => onOpenOrder(o)}
              onPrintInvoice={() => setInvoiceOrder(o)}
              onEditShipping={() => setShippingOrder(o)}
              onEditStatus={() => setStatusOrder(o)}
              onRecordPayment={() => setPaymentActionOrder(o)}
              now={now}
            />
          ))}
        </div>
      ) : (
        // Compact Table/List View
        <>
          <div className="hidden md:grid grid-cols-[150px_minmax(0,1.3fr)_minmax(0,1.6fr)_110px_minmax(0,auto)_20px] gap-x-4 px-4 text-[10px] font-extrabold uppercase tracking-wider text-slate-400">
            <span>Order · IST</span>
            <span>Customer</span>
            <span>Items</span>
            <span className="text-right">Total</span>
            <span>Status · Payment</span>
            <span />
          </div>
          <ul className={`space-y-2 transition-opacity ${orders.isPlaceholderData ? 'opacity-60' : ''}`} aria-label="Orders">
            {rows.map((o) => (
              <OrderRow key={o.id} order={o} highlighted={highlighted.has(o.orderNumber)} onOpen={() => onOpenOrder(o)} now={now} />
            ))}
          </ul>
        </>
      )}

      {/* Infinite Sentinel / Pagination Footer */}
      {rows.length > 0 && (
        <div ref={sentinel} className="flex flex-col items-center gap-2 py-4">
          {orders.isFetchNextPageError && (
            <p role="alert" className="text-xs font-bold text-rose-600">
              Could not load more orders.
            </p>
          )}
          {orders.hasNextPage ? (
            <Button onClick={() => void orders.fetchNextPage()} loading={orders.isFetchingNextPage}>
              Load more
            </Button>
          ) : (
            <p className="text-[11px] text-slate-400 font-semibold">
              {rows.length} order{rows.length === 1 ? '' : 's'} · end of list
            </p>
          )}
          {orders.isFetching && !orders.isFetchingNextPage && <Spinner label="Refreshing…" />}
        </div>
      )}

      {/* Official Order Estimation & Printable Slip Modal */}
      {invoiceOrder && (
        <OrderInvoiceModal order={invoiceOrder} onClose={() => setInvoiceOrder(null)} />
      )}

      {/* Transport & Docket LR Modal */}
      {shippingOrder && (
        <OrderShippingDialog
          order={shippingOrder}
          open={!!shippingOrder}
          onClose={() => setShippingOrder(null)}
        />
      )}

      {/* Provide Status Modal */}
      {statusOrder && (
        <OrderStatusDialog
          order={statusOrder}
          open={!!statusOrder}
          onClose={() => setStatusOrder(null)}
        />
      )}

      {/* Record Payment Dialog */}
      {paymentActionOrder && (
        <OrderPaymentDialog
          order={paymentActionOrder}
          action="mark_paid"
          onClose={() => setPaymentActionOrder(null)}
        />
      )}
    </div>
  );
}
