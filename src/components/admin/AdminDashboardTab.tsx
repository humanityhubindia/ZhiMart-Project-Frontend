import React from 'react';
import {
  AlertTriangle,
  ArrowUpRight,
  CalendarDays,
  ChevronRight,
  Clock,
  FlaskConical,
  Hourglass,
  CheckCircle2,
  Mail,
  IndianRupee,
  PackageX,
  ShoppingCart,
  TrendingUp,
  Wallet,
} from 'lucide-react';
import { useDashboard, useStorefrontConfig } from '../../hooks/admin/adminQueries';
import type { Order, OrderStatus } from '../../types/admin';
import { SalesChart } from './SalesChart';
import { formatCount, formatDateTime, formatINR, itemsSummary, type AdminTab } from './format';
import {
  EmptyState,
  ErrorCard,
  KpiCard,
  OrderStatusBadge,
  PaymentStatusBadge,
  Skeleton,
  TestBadge,
} from './ui';

interface Props {
  onOpenOrder: (order: Order) => void;
  onNavigate: (tab: AdminTab) => void;
  onShowInventoryProduct: (productId: string) => void;
  onShowOrders: (status: OrderStatus | null) => void;
}

function DashboardSkeleton() {
  return (
    <div className="space-y-6" aria-busy="true" aria-label="Loading dashboard">
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        {Array.from({ length: 8 }).map((_, i) => (
          <Skeleton key={i} className="h-32 rounded-3xl" />
        ))}
      </div>
      <Skeleton className="h-80 rounded-3xl" />
      <div className="grid lg:grid-cols-2 gap-6">
        <Skeleton className="h-64 rounded-3xl" />
        <Skeleton className="h-64 rounded-3xl" />
      </div>
    </div>
  );
}

export function AdminDashboardTab({ onOpenOrder, onNavigate, onShowInventoryProduct, onShowOrders }: Props) {
  const dashboard = useDashboard();
  const config = useStorefrontConfig();
  const onlinePayments = config.data?.onlinePaymentsEnabled !== false;

  if (dashboard.isPending) return <DashboardSkeleton />;
  if (dashboard.isError && !dashboard.data) {
    return (
      <ErrorCard
        title="Could not load the dashboard"
        error={dashboard.error}
        onRetry={() => void dashboard.refetch()}
        retrying={dashboard.isFetching}
      />
    );
  }

  const d = dashboard.data;
  const maxUnits = Math.max(1, ...d.topProducts.map((p) => p.unitsSold));

  return (
    <div className="space-y-6">
      <div className="flex flex-col sm:flex-row sm:items-end justify-between gap-3">
        <div>
          <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">Dashboard</h2>
          <p className="text-xs text-slate-500 font-semibold mt-0.5">
            Live figures from the order database. Revenue counts paid-for real orders only — test orders, unpaid
            checkouts, cancellations and refunds are excluded. Days are in IST.
          </p>
          <p className="text-[11px] text-slate-400 mt-1 flex items-center gap-1">
            <Mail className="w-3 h-3" aria-hidden /> New-order emails go to the admin login emails (or ORDER_NOTIFICATION_EMAILS if set on the server).
          </p>
          {d.test && d.test.orders > 0 && (
            <p className="mt-2 inline-flex items-center gap-1.5 text-[11px] font-bold text-violet-800 bg-violet-50 border border-violet-200 px-2.5 py-1 rounded-lg">
              <FlaskConical className="w-3.5 h-3.5" />
              Test orders: {formatCount(d.test.orders)} / {formatINR(d.test.revenue)} (excluded)
            </p>
          )}
        </div>
        {dashboard.isError && (
          <p role="status" className="text-xs font-bold text-amber-700 flex items-center gap-1.5">
            <AlertTriangle className="w-3.5 h-3.5" /> Showing last loaded figures — refresh failed.
          </p>
        )}
      </div>

      {/* KPIs */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <KpiCard
          label="Revenue today"
          value={formatINR(d.revenue.today)}
          icon={IndianRupee}
          tone="amber"
          hint={`${formatCount(d.orders.today)} order${d.orders.today === 1 ? '' : 's'} today`}
        />
        <KpiCard
          label="Last 7 days"
          value={formatINR(d.revenue.last7Days)}
          icon={TrendingUp}
          tone="emerald"
          hint={`30 days: ${formatINR(d.revenue.last30Days)}`}
        />
        <KpiCard
          label="Pending collection"
          value={formatINR(d.revenue.pendingCollection)}
          icon={Wallet}
          tone="purple"
          hint={`Collected: ${formatINR(d.revenue.paid)}`}
          onClick={() => onNavigate('payments')}
        />
        <KpiCard
          label="Orders today"
          value={formatCount(d.orders.today)}
          icon={CalendarDays}
          tone="blue"
          hint={`${formatCount(d.orders.total)} orders all time`}
        />
        {onlinePayments || (d.unpaid?.orders ?? 0) > 0 ? (
          <KpiCard
            label="Awaiting payment"
            value={formatCount(d.unpaid?.orders ?? d.orders.byStatus.pending_payment ?? 0)}
            icon={Hourglass}
            tone="blue"
            hint={`${formatINR(d.unpaid?.amount ?? 0)} of stock reserved`}
            onClick={() => onShowOrders('pending_payment')}
          />
        ) : (
          <KpiCard
            label="Collected"
            value={formatINR(d.revenue.paid)}
            icon={CheckCircle2}
            tone="emerald"
            hint="Payments recorded as received"
            onClick={() => onNavigate('payments')}
          />
        )}
        <KpiCard
          label="Pending pack"
          value={formatCount(d.orders.byStatus.pending_pack ?? 0)}
          icon={Clock}
          tone="amber"
          hint="Awaiting packing & dispatch"
          onClick={() => onShowOrders('pending_pack')}
        />
        <KpiCard
          label="Low stock"
          value={formatCount(d.inventory.lowStockCount)}
          icon={AlertTriangle}
          tone="amber"
          hint="At or below threshold"
          onClick={() => onNavigate('inventory')}
        />
        <KpiCard
          label="Out of stock"
          value={formatCount(d.inventory.outOfStockCount)}
          icon={PackageX}
          tone="rose"
          hint={`${formatCount(d.inventory.totalUnits)} units across ${formatCount(d.inventory.activeProducts)} active products`}
          onClick={() => onNavigate('inventory')}
        />
      </div>

      <SalesChart days={d.salesByDay} />

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Top products */}
        <section className="bg-white p-5 sm:p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
          <h3 className="text-base font-extrabold text-slate-950">Top products · 30 days</h3>
          <p className="text-xs text-slate-500 mt-0.5 mb-4">By units sold.</p>
          {d.topProducts.length === 0 ? (
            <p className="text-xs text-slate-500 py-6 text-center">No sales in the last 30 days.</p>
          ) : (
            <ol className="space-y-3">
              {d.topProducts.map((p, i) => (
                <li key={p.productId}>
                  <div className="flex items-baseline justify-between gap-3 text-xs">
                    <span className="font-extrabold text-slate-900 truncate">
                      <span className="text-slate-400 mr-1.5 tabular-nums">{i + 1}.</span>
                      {p.name}
                    </span>
                    <span className="shrink-0 tabular-nums text-slate-600 font-semibold">
                      {formatCount(p.unitsSold)} units · <span className="text-slate-900 font-extrabold">{formatINR(p.revenue)}</span>
                    </span>
                  </div>
                  <div className="mt-1.5 h-1.5 rounded-full bg-slate-100 overflow-hidden" aria-hidden>
                    <div className="h-full rounded-full bg-amber-500" style={{ width: `${(p.unitsSold / maxUnits) * 100}%` }} />
                  </div>
                </li>
              ))}
            </ol>
          )}
        </section>

        {/* Low stock */}
        <section className="bg-white p-5 sm:p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
          <div className="flex items-start justify-between gap-3 mb-4">
            <div>
              <h3 className="text-base font-extrabold text-slate-950">Low stock</h3>
              <p className="text-xs text-slate-500 mt-0.5">Lowest first. Click to manage stock.</p>
            </div>
            <button
              type="button"
              onClick={() => onNavigate('inventory')}
              className="text-xs font-bold text-amber-700 hover:text-amber-900 inline-flex items-center gap-1"
            >
              Inventory <ArrowUpRight className="w-3.5 h-3.5" />
            </button>
          </div>
          {d.inventory.lowStock.length === 0 ? (
            <p className="text-xs text-slate-500 py-6 text-center">All products are above their low-stock threshold.</p>
          ) : (
            <ul className="divide-y divide-slate-100">
              {d.inventory.lowStock.map((p) => (
                <li key={p.productId}>
                  <button
                    type="button"
                    onClick={() => onShowInventoryProduct(p.productId)}
                    className="w-full flex items-center justify-between gap-3 py-2.5 text-left hover:bg-slate-50 rounded-lg px-2 -mx-2"
                  >
                    <span className="min-w-0">
                      <span className="block text-xs font-extrabold text-slate-900 truncate">{p.name}</span>
                      <span className="block text-[11px] text-slate-400 font-mono">{p.productId}</span>
                    </span>
                    <span className="flex items-center gap-2 shrink-0">
                      <span
                        className={`text-xs font-black tabular-nums ${p.stock === 0 ? 'text-rose-600' : 'text-amber-700'}`}
                      >
                        {p.stock === 0 ? 'Out of stock' : `${formatCount(p.stock)} left`}
                      </span>
                      <span className="text-[11px] text-slate-400 tabular-nums">/ {formatCount(p.lowStockThreshold)}</span>
                      <ChevronRight className="w-3.5 h-3.5 text-slate-300" />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      {/* Recent orders */}
      <section className="bg-white p-5 sm:p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
        <div className="flex items-start justify-between gap-3 mb-4">
          <div>
            <h3 className="text-base font-extrabold text-slate-950">Recent orders</h3>
            <p className="text-xs text-slate-500 mt-0.5">Latest five. Click an order for details and actions.</p>
          </div>
          <button
            type="button"
            onClick={() => onShowOrders(null)}
            className="text-xs font-bold text-amber-700 hover:text-amber-900 inline-flex items-center gap-1"
          >
            All orders <ArrowUpRight className="w-3.5 h-3.5" />
          </button>
        </div>
        {d.recentOrders.length === 0 ? (
          <EmptyState icon={ShoppingCart} title="No orders yet" description="New storefront orders appear here automatically." />
        ) : (
          <ul className="divide-y divide-slate-100">
            {d.recentOrders.map((o) => (
              <li key={o.id}>
                <button
                  type="button"
                  onClick={() => onOpenOrder(o)}
                  className="w-full grid grid-cols-1 sm:grid-cols-[1fr_auto] gap-2 py-3 text-left hover:bg-slate-50 rounded-lg px-2 -mx-2"
                >
                  <span className="min-w-0">
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-sm font-black text-slate-950">{o.orderNumber}</span>
                      <OrderStatusBadge status={o.status} />
                      <PaymentStatusBadge status={o.payment.status} />
                      {o.isTest && <TestBadge />}
                    </span>
                    <span className="block text-xs text-slate-500 mt-1 truncate">
                      {o.customer.name} · {o.customer.city} · {itemsSummary(o)}
                    </span>
                  </span>
                  <span className="sm:text-right">
                    <span className="block text-sm font-black text-slate-950 tabular-nums">{formatINR(o.totalAmount)}</span>
                    <span className="block text-[11px] text-slate-400">{formatDateTime(o.createdAt)}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
