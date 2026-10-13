import React, { useMemo, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  CreditCard,
  FlaskConical,
  Hourglass,
  Info,
  RotateCcw,
  Search,
  Undo2,
  Wallet,
  X,
  XCircle,
} from 'lucide-react';
import { useAdminOrders, useDashboard, useStorefrontConfig } from '../../hooks/admin/adminQueries';
import { useDebouncedValue } from '../../hooks/admin/useDebouncedValue';
import type { Order, PaymentAction, PaymentStatus } from '../../types/admin';
import { PAYMENT_STATUSES } from '../../types/admin';
import {
  formatCount,
  formatDateTime,
  formatINR,
  PAYMENT_STATUS_META,
  paymentMethodSummary,
  razorpayPaymentUrl,
} from './format';
import {
  availablePaymentActions,
  canRefundOnline,
  OrderPaymentDialog,
  OrderRefundDialog,
  PAYMENT_ACTION_LABEL,
} from './OrderActionDialogs';
import { OrderRowsSkeleton } from './AdminOrdersTab';
import { RazorpayId } from './PaymentDetails';
import {
  AttentionBadge,
  Button,
  EmptyState,
  ErrorCard,
  FilterChip,
  inputClass,
  KpiCard,
  OrderStatusBadge,
  PaymentStatusBadge,
  Skeleton,
  Spinner,
  TestBadge,
} from './ui';
import { useInfiniteSentinel } from './useInfiniteSentinel';

interface Props {
  onOpenOrder: (order: Order) => void;
}

function RefundedSummary() {
  // The dashboard has no refunded total: sum refunds on fully refunded orders (latest 100).
  const refunded = useAdminOrders({ paymentStatus: 'refunded', includeUnpaid: true, limit: 100 });
  const first = refunded.data?.pages[0];
  if (refunded.isPending) return <Skeleton className="h-[124px] rounded-3xl" />;
  const total = first?.orders.reduce((a, o) => a + (o.payment.amountRefunded || o.totalAmount), 0) ?? 0;
  const count = first?.orders.length ?? 0;
  const more = !!first?.nextCursor;
  return (
    <KpiCard
      label="Refunded"
      value={refunded.isError ? '—' : formatINR(total)}
      icon={Undo2}
      tone="rose"
      hint={
        refunded.isError
          ? 'Could not load refunds'
          : `${count}${more ? '+' : ''} fully refunded order${count === 1 ? '' : 's'}${more ? ' (latest 100)' : ''}`
      }
    />
  );
}

type ActiveDialog = { order: Order; kind: 'refund' } | { order: Order; kind: 'manual'; action: PaymentAction };

export function AdminPaymentsTab({ onOpenOrder }: Props) {
  const [status, setStatus] = useState<PaymentStatus | null>(null);
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [hideTest, setHideTest] = useState(false);
  const [search, setSearch] = useState('');
  const q = useDebouncedValue(search.trim(), 350);
  const [dialog, setDialog] = useState<ActiveDialog | null>(null);

  const dashboard = useDashboard();
  const config = useStorefrontConfig();
  const onlinePayments = config.data?.onlinePaymentsEnabled !== false;
  // Payments view includes orders still awaiting online payment.
  const orders = useAdminOrders({ paymentStatus: status, q, includeUnpaid: true });
  const loaded = useMemo(() => orders.data?.pages.flatMap((p) => p.orders) ?? [], [orders.data]);
  const rows = useMemo(
    () => loaded.filter((o) => (!attentionOnly || !!o.payment.attention) && (!hideTest || !o.isTest)),
    [loaded, attentionOnly, hideTest]
  );
  const clientFiltered = attentionOnly || hideTest;
  const sentinel = useInfiniteSentinel(
    !!orders.hasNextPage && !orders.isFetchingNextPage && !orders.isFetchNextPageError,
    () => void orders.fetchNextPage()
  );
  const anyFilter = !!status || !!q || clientFiltered;
  const d = dashboard.data;
  const showUnpaidCard = onlinePayments || (d?.unpaid?.orders ?? 0) > 0;

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">Payments</h2>
        <p className="text-xs text-slate-500 font-semibold mt-0.5">
          {onlinePayments
            ? 'Online payments are confirmed by Razorpay (verified signature or webhook) — never by the browser. Refund paid Razorpay orders here; pay-later orders are recorded manually.'
            : 'Online payments are off — every order is pay-later. Collect by UPI, bank transfer or cash, then use “Record payment” with the UTR / reference.'}
        </p>
      </div>

      <div className={`grid grid-cols-1 sm:grid-cols-2 gap-4 ${showUnpaidCard ? 'lg:grid-cols-4' : 'lg:grid-cols-3'}`}>
        {dashboard.isPending ? (
          <>
            <Skeleton className="h-[124px] rounded-3xl" />
            <Skeleton className="h-[124px] rounded-3xl" />
            <Skeleton className="h-[124px] rounded-3xl" />
          </>
        ) : dashboard.isError && !d ? (
          <div className="sm:col-span-2 lg:col-span-3">
            <ErrorCard compact error={dashboard.error} onRetry={() => void dashboard.refetch()} title="Could not load totals" />
          </div>
        ) : d ? (
          <>
            <KpiCard
              label="Collected"
              value={formatINR(d.revenue.paid)}
              icon={CheckCircle2}
              tone="emerald"
              hint="Real orders; excludes test, cancelled & refunded"
              onClick={() => setStatus('paid')}
            />
            <KpiCard
              label="Pending collection"
              value={formatINR(d.revenue.pendingCollection)}
              icon={Wallet}
              tone="amber"
              hint={onlinePayments ? 'Confirmed orders not yet paid' : 'Pay-later orders to collect — click to list'}
              onClick={() => setStatus('pending')}
            />
            {showUnpaidCard && (
            <KpiCard
              label="Awaiting payment"
              value={formatINR(d.unpaid?.amount ?? 0)}
              icon={Hourglass}
              tone="blue"
              hint={`${formatCount(d.unpaid?.orders ?? 0)} checkout${(d.unpaid?.orders ?? 0) === 1 ? '' : 's'} holding reserved stock`}
              onClick={() => setStatus('pending')}
            />
            )}
          </>
        ) : null}
        <RefundedSummary />
      </div>

      {d?.test && d.test.orders > 0 && (
        <p className="inline-flex items-center gap-1.5 text-[11px] font-bold text-violet-800 bg-violet-50 border border-violet-200 px-2.5 py-1 rounded-lg">
          <FlaskConical className="w-3.5 h-3.5" />
          Test orders: {formatCount(d.test.orders)} / {formatINR(d.test.revenue)} — not counted above
        </p>
      )}

      <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2" role="group" aria-label="Filter payments">
          <FilterChip active={status === null} onClick={() => setStatus(null)}>
            All
          </FilterChip>
          {PAYMENT_STATUSES.map((s) => (
            <FilterChip key={s} active={status === s} onClick={() => setStatus(s)}>
              {PAYMENT_STATUS_META[s].label}
            </FilterChip>
          ))}
          <FilterChip active={attentionOnly} onClick={() => setAttentionOnly((v) => !v)}>
            <AlertTriangle className="w-3 h-3" /> Needs attention
          </FilterChip>
          <FilterChip active={hideTest} onClick={() => setHideTest((v) => !v)}>
            Hide test
          </FilterChip>
        </div>
        <div className="relative lg:w-80">
          <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" aria-hidden />
          <label htmlFor="payments-search" className="sr-only">
            Search payments
          </label>
          <input
            id="payments-search"
            type="search"
            maxLength={100}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Order no., name, phone or city"
            className={`${inputClass} pl-10`}
          />
        </div>
      </div>

      {clientFiltered && (
        <p className="flex items-center gap-1.5 text-[11px] text-slate-500">
          <Info className="w-3.5 h-3.5" />
          “Needs attention” and “Hide test” apply to the {formatCount(loaded.length)} orders loaded so far
          {orders.hasNextPage ? ' — load more to search further back.' : '.'}
        </p>
      )}

      {orders.isPending ? (
        <OrderRowsSkeleton />
      ) : orders.isError && loaded.length === 0 ? (
        <ErrorCard title="Could not load payments" error={orders.error} onRetry={() => void orders.refetch()} retrying={orders.isFetching} />
      ) : rows.length === 0 && !orders.hasNextPage ? (
        <EmptyState
          icon={CreditCard}
          title={attentionOnly ? 'No payments need attention' : 'No payments match'}
          description={anyFilter ? 'Try another filter or search.' : 'Payments appear as orders are placed.'}
          action={
            anyFilter ? (
              <Button
                icon={X}
                onClick={() => {
                  setStatus(null);
                  setSearch('');
                  setAttentionOnly(false);
                  setHideTest(false);
                }}
              >
                Clear filters
              </Button>
            ) : undefined
          }
        />
      ) : (
        <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden">
          <div className="overflow-x-auto">
            <table className={`w-full text-xs ${orders.isPlaceholderData ? 'opacity-60' : ''}`}>
              <thead className="bg-slate-50 text-[10px] uppercase tracking-wider text-slate-500 text-left">
                <tr>
                  <th scope="col" className="px-4 py-3 font-extrabold">Order</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Customer</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Amount</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Method</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Status</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Payment id / ref</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Paid / refunded</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {rows.map((o) => {
                  const manual = availablePaymentActions(o);
                  const refundable = canRefundOnline(o);
                  const refunded = o.payment.amountRefunded ?? 0;
                  return (
                    <tr key={o.id} className={`align-top ${o.payment.attention ? 'bg-rose-50/60' : 'hover:bg-slate-50/60'}`}>
                      <td className="px-4 py-3">
                        <button
                          type="button"
                          onClick={() => onOpenOrder(o)}
                          className="font-mono font-black text-slate-950 hover:text-amber-700 hover:underline"
                        >
                          {o.orderNumber}
                        </button>
                        <p className="text-[11px] text-slate-400 mt-0.5 whitespace-nowrap">{formatDateTime(o.createdAt)}</p>
                        <div className="mt-1 flex flex-wrap gap-1">
                          {(o.status === 'cancelled' || o.status === 'pending_payment') && <OrderStatusBadge status={o.status} />}
                          {o.isTest && <TestBadge />}
                        </div>
                      </td>
                      <td className="px-4 py-3">
                        <p className="font-extrabold text-slate-900">{o.customer.name}</p>
                        <p className="text-[11px] text-slate-500">{o.customer.phone}</p>
                      </td>
                      <td className="px-4 py-3 text-right tabular-nums whitespace-nowrap">
                        <p className="font-black text-slate-950">{formatINR(o.totalAmount)}</p>
                        {refunded > 0 && <p className="text-[11px] font-bold text-rose-700">−{formatINR(refunded)} refunded</p>}
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">{paymentMethodSummary(o.payment)}</td>
                      <td className="px-4 py-3">
                        <div className="flex flex-col items-start gap-1">
                          <PaymentStatusBadge status={o.payment.status} />
                          {o.payment.attention && <AttentionBadge title={o.payment.attention} />}
                        </div>
                      </td>
                      <td className="px-4 py-3 max-w-[200px]">
                        {o.payment.razorpayPaymentId ? (
                          <RazorpayId
                            id={o.payment.razorpayPaymentId}
                            href={razorpayPaymentUrl(o.payment.razorpayPaymentId)}
                            label="Razorpay payment id"
                          />
                        ) : o.payment.reference ? (
                          <span className="font-mono text-[11px] text-slate-700 break-all">{o.payment.reference}</span>
                        ) : (
                          <span className="text-slate-300">—</span>
                        )}
                      </td>
                      <td className="px-4 py-3 text-[11px] text-slate-600 whitespace-nowrap">
                        {o.payment.refundedAt ? (
                          <>Refunded {formatDateTime(o.payment.refundedAt)}</>
                        ) : o.payment.paidAt ? (
                          <>Paid {formatDateTime(o.payment.paidAt)}</>
                        ) : (
                          <span className="text-slate-300">—</span>
                        )}
                      </td>
                      <td className="px-4 py-3">
                        <div className="flex justify-end gap-1.5">
                          {refundable && (
                            <Button
                              size="sm"
                              variant="dangerOutline"
                              icon={RotateCcw}
                              onClick={() => setDialog({ order: o, kind: 'refund' })}
                              aria-label={`Refund ${o.orderNumber}`}
                            >
                              Refund
                            </Button>
                          )}
                          {manual.map((a) => (
                            <Button
                              key={a}
                              size="sm"
                              variant={a === 'mark_paid' ? 'success' : 'dangerOutline'}
                              icon={a === 'mark_paid' ? CheckCircle2 : a === 'mark_refunded' ? Undo2 : XCircle}
                              onClick={() => setDialog({ order: o, kind: 'manual', action: a })}
                              aria-label={`${PAYMENT_ACTION_LABEL[a]} for ${o.orderNumber}`}
                            >
                              {PAYMENT_ACTION_LABEL[a]}
                            </Button>
                          ))}
                          {!refundable && manual.length === 0 && <span className="text-[11px] text-slate-300">—</span>}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {rows.length === 0 && (
              <p className="px-4 py-6 text-center text-xs text-slate-500">No matches among the loaded orders.</p>
            )}
          </div>
          <div ref={sentinel} className="flex flex-col items-center gap-2 py-4 border-t border-slate-100">
            {orders.isFetchNextPageError && (
              <p role="alert" className="text-xs font-bold text-rose-600">
                Could not load more.
              </p>
            )}
            {orders.hasNextPage ? (
              <Button onClick={() => void orders.fetchNextPage()} loading={orders.isFetchingNextPage}>
                Load more
              </Button>
            ) : (
              <p className="text-[11px] text-slate-400 font-semibold">{rows.length} shown · end of list</p>
            )}
            {orders.isFetching && !orders.isFetchingNextPage && <Spinner label="Refreshing…" />}
          </div>
        </div>
      )}

      {dialog?.kind === 'manual' && (
        <OrderPaymentDialog order={dialog.order} action={dialog.action} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'refund' && <OrderRefundDialog order={dialog.order} open onClose={() => setDialog(null)} />}
    </div>
  );
}
