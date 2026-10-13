import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  ArrowLeft,
  Clock,
  CreditCard,
  FlaskConical,
  ExternalLink,
  LayoutDashboard,
  Loader2,
  LogOut,
  Package,
  ShoppingBag,
  Wallet,
  Warehouse,
  X,
} from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { useAdminAuth } from '../auth/AdminAuthContext';
import { AdminLoginForm } from './AdminLoginForm';
import { adminKeys, useDashboard, useStorefrontConfig } from '../hooks/admin/adminQueries';
import { useAdminSync, type SyncHealth } from '../hooks/admin/useAdminSync';
import type { Order, OrderStatus } from '../types/admin';
import { ToastProvider, useToast } from './admin/Toast';
import { AdminDashboardTab } from './admin/AdminDashboardTab';
import { AdminOrdersTab } from './admin/AdminOrdersTab';
import { AdminProductsTab } from './admin/AdminProductsTab';
import { AdminPaymentsTab } from './admin/AdminPaymentsTab';
import { AdminInventoryTab } from './admin/AdminInventoryTab';
import { AdminOrderDetailPage } from './admin/AdminOrderDetailPage';
import { OrderDetailDrawer, type OrderTarget } from './admin/OrderDetailDrawer';
import { formatDateTime, formatRelative, type AdminTab } from './admin/format';

interface AdminPageProps {
  onBackToStore: () => void;
}

/** How long a newly-arrived order stays highlighted in the list. */
const NEW_ORDER_HIGHLIGHT_MS = 60_000;

const BrandMark: React.FC<{ size?: 'sm' | 'lg' }> = ({ size = 'sm' }) =>
  size === 'lg' ? (
    <div className="w-14 h-14 rounded-2xl bg-gradient-to-tr from-slate-950 to-slate-800 p-1 shadow-md mb-3 flex items-center justify-center">
      <span
        className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-2xl select-none"
        style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
      >
        ழி
      </span>
    </div>
  ) : (
    <div className="w-10 h-10 rounded-2xl bg-gradient-to-tr from-slate-900 via-slate-800 to-amber-500 p-0.5 shadow-xs shrink-0">
      <div className="w-full h-full bg-slate-950 rounded-[14px] flex items-center justify-center">
        <span
          className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-lg leading-none"
          style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
        >
          ழி
        </span>
      </div>
    </div>
  );

// ---------------------------------------------------------------------------
// Live indicator
// ---------------------------------------------------------------------------

function LiveIndicator({ health, lastSyncedAt }: { health: SyncHealth; lastSyncedAt: number | null }) {
  const meta = {
    live: { dot: 'bg-emerald-500', ping: 'bg-emerald-400', text: 'text-emerald-800', box: 'bg-emerald-50 border-emerald-200', label: 'Live' },
    failing: { dot: 'bg-amber-500', ping: '', text: 'text-amber-800', box: 'bg-amber-50 border-amber-200', label: 'Reconnecting' },
    connecting: { dot: 'bg-slate-400', ping: '', text: 'text-slate-600', box: 'bg-slate-50 border-slate-200', label: 'Connecting' },
  }[health];
  const title =
    health === 'live'
      ? `Live updates on · last checked ${formatRelative(lastSyncedAt)}`
      : health === 'failing'
        ? `Live updates are failing — retrying every 5 s. Last success: ${lastSyncedAt ? formatDateTime(new Date(lastSyncedAt).toISOString()) : 'never'}`
        : 'Connecting to live updates…';
  return (
    <div
      role="status"
      aria-label={title}
      title={title}
      className={`inline-flex items-center gap-2 border px-2.5 py-1.5 rounded-xl ${meta.box}`}
    >
      <span className="relative flex w-2 h-2">
        {meta.ping && <span className={`absolute inline-flex h-full w-full rounded-full opacity-60 animate-ping ${meta.ping}`} />}
        <span className={`relative inline-flex w-2 h-2 rounded-full ${meta.dot}`} />
      </span>
      <span className={`text-[11px] font-extrabold ${meta.text}`}>{meta.label}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Payment mode bar (from public GET /storefront/config)
// ---------------------------------------------------------------------------

function PaymentModeBar() {
  const config = useStorefrontConfig();
  const c = config.data;
  if (!c) return null;
  // Older backends don't send the flag; treat a missing value as "on" (previous behaviour).
  const online = c.onlinePaymentsEnabled !== false;
  const keyId = c.razorpayKeyId ?? '';
  const keyKind = keyId.startsWith('rzp_test_') ? 'test' : keyId.startsWith('rzp_live_') ? 'live' : 'unknown';
  const keyLabel =
    keyKind === 'test'
      ? 'Razorpay TEST keys — no real money moves'
      : keyKind === 'live'
        ? 'Razorpay LIVE keys — real ₹1 charges'
        : 'Razorpay keys not configured';

  return (
    <>
      {!online && (
        <div role="status" className="bg-slate-800 text-slate-100 border-b border-slate-900">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-1.5 flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-[11px] font-bold">
            <span className="inline-flex items-center gap-1.5">
              <Wallet className="w-3.5 h-3.5 text-slate-300" />
              Online payments OFF — orders are pay-later; record payments manually
            </span>
          </div>
        </div>
      )}
      {c.testMode && (
        <div role="status" className="bg-amber-400 text-slate-950 border-b border-amber-500">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-1.5 flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-[11px] font-black uppercase tracking-wide">
            <span className="inline-flex items-center gap-1.5">
              <FlaskConical className="w-3.5 h-3.5" />
              Payment test mode — customers are charged ₹1 per item
            </span>
            {online && (
              <span className={`normal-case tracking-normal font-extrabold ${keyKind === 'live' ? 'text-rose-800' : ''}`}>
                {keyLabel}
              </span>
            )}
          </div>
        </div>
      )}
      {online && !c.testMode && keyKind !== 'live' && (
        // Real prices with online payments on, but not wired to live keys: payments won't settle.
        <div role="status" className="bg-slate-800 text-amber-300 border-b border-slate-900">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-1.5 text-[11px] font-extrabold">
            Real prices are on, but{' '}
            {keyKind === 'test' ? 'Razorpay is using TEST keys — payments are simulated.' : 'no Razorpay key is configured.'}
          </div>
        </div>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Console (authenticated)
// ---------------------------------------------------------------------------

const TABS: { id: AdminTab; label: string; icon: React.ComponentType<{ className?: string }> }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { id: 'orders', label: 'Orders', icon: Package },
  { id: 'products', label: 'Products', icon: ShoppingBag },
  { id: 'payments', label: 'Payments', icon: CreditCard },
  { id: 'inventory', label: 'Inventory', icon: Warehouse },
];

function AdminConsole({ onBackToStore }: AdminPageProps) {
  const { admin, logout } = useAdminAuth();
  const toast = useToast();
  const [activeTab, setActiveTab] = useState<AdminTab>('dashboard');
  const [orderStatus, setOrderStatus] = useState<OrderStatus | null>(null);
  const [orderTarget, setOrderTarget] = useState<OrderTarget | null>(null);
  const [inventoryFocus, setInventoryFocus] = useState<string | null>(null);
  const [highlighted, setHighlighted] = useState<ReadonlySet<string>>(() => new Set());
  const highlightTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const [loggingOut, setLoggingOut] = useState(false);

  useEffect(() => {
    const handleHash = () => {
      const hash = window.location.hash;
      if (hash.startsWith('#order=')) {
        const orderNum = decodeURIComponent(hash.slice(7));
        if (orderNum) setOrderTarget({ orderNumber: orderNum });
      } else if (!hash) {
        setOrderTarget(null);
      }
    };
    handleHash();
    window.addEventListener('hashchange', handleHash);
    return () => window.removeEventListener('hashchange', handleHash);
  }, []);

  const openOrder = useCallback((order: Order) => {
    setOrderTarget({ id: order.id, seed: order });
    window.location.hash = `order=${order.orderNumber}`;
  }, []);

  const openOrderNumber = useCallback((orderNumber: string) => {
    setOrderTarget({ orderNumber });
    window.location.hash = `order=${orderNumber}`;
  }, []);

  const handleBackFromDetail = useCallback(() => {
    setOrderTarget(null);
    if (window.location.hash.startsWith('#order=')) {
      window.history.pushState(null, '', window.location.pathname + window.location.search);
    }
  }, []);

  const onNewOrder = useCallback(
    (orderNumber: string) => {
      setHighlighted((prev) => new Set(prev).add(orderNumber));
      const existing = highlightTimers.current.get(orderNumber);
      if (existing) clearTimeout(existing);
      highlightTimers.current.set(
        orderNumber,
        setTimeout(() => {
          highlightTimers.current.delete(orderNumber);
          setHighlighted((prev) => {
            const next = new Set(prev);
            next.delete(orderNumber);
            return next;
          });
        }, NEW_ORDER_HIGHLIGHT_MS)
      );
      toast.order(`New order ${orderNumber} received`, { label: 'View', onClick: () => openOrderNumber(orderNumber) });
    },
    [toast, openOrderNumber]
  );

  const sync = useAdminSync(onNewOrder);
  const dashboard = useDashboard();
  const pendingCount = sync.data?.pendingOrders ?? dashboard.data?.orders.byStatus.pending_pack ?? 0;
  const stockAlerts = dashboard.data ? dashboard.data.inventory.lowStockCount + dashboard.data.inventory.outOfStockCount : 0;

  const showOrders = useCallback((status: OrderStatus | null) => {
    setOrderStatus(status);
    setActiveTab('orders');
  }, []);
  const showInventoryProduct = useCallback((productId: string) => {
    setInventoryFocus(productId);
    setActiveTab('inventory');
  }, []);
  const clearInventoryFocus = useCallback(() => setInventoryFocus(null), []);

  const handleLogout = async () => {
    setLoggingOut(true);
    try {
      await logout();
    } finally {
      setLoggingOut(false);
    }
  };

  return (
    <div className="min-h-screen bg-[#F8F9FA] text-slate-900 flex flex-col text-left">
      <header className="bg-white border-b border-slate-200/90 sticky top-0 z-30 shadow-xs">
        <PaymentModeBar />
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 sm:h-20 flex items-center justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <BrandMark />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-lg font-black tracking-tight text-slate-900 font-heading">ZHI-MART</span>
                <span className="bg-amber-100 text-amber-900 text-[10px] font-black uppercase px-2 py-0.5 rounded-md border border-amber-200/60">
                  Admin
                </span>
              </div>
              <p className="text-[10px] font-bold text-slate-400 hidden sm:block">Orders, catalogue & inventory console</p>
            </div>
          </div>

          <div className="flex items-center gap-2 sm:gap-3">
            <LiveIndicator health={sync.health} lastSyncedAt={sync.lastSyncedAt} />
            <button
              type="button"
              onClick={onBackToStore}
              className="inline-flex items-center gap-1.5 text-xs font-bold text-slate-700 hover:text-slate-900 bg-slate-100 hover:bg-slate-200/80 px-3 py-2 rounded-xl transition-all"
              title="Open the customer storefront"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">Preview Storefront</span>
            </button>
            <div className="h-6 w-px bg-slate-200 hidden md:block" />
            <span
              className="text-xs font-extrabold text-slate-700 hidden md:inline max-w-[220px] truncate"
              title={admin?.displayName ?? undefined}
            >
              {admin?.email}
            </span>
            <button
              type="button"
              onClick={() => void handleLogout()}
              disabled={loggingOut}
              className="inline-flex items-center gap-1.5 text-xs font-bold text-rose-600 hover:bg-rose-50 border border-rose-200 px-3 py-2 rounded-xl transition-all disabled:opacity-50"
              title="Sign out of the admin session"
            >
              {loggingOut ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <LogOut className="w-3.5 h-3.5" />}
              <span className="hidden sm:inline">Logout</span>
            </button>
          </div>
        </div>

        <nav
          aria-label="Admin sections"
          className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex overflow-x-auto gap-2 py-2 border-t border-slate-100 no-scrollbar"
        >
          {TABS.map(({ id, label, icon: Icon }) => {
            const active = activeTab === id;
            const badge =
              id === 'orders' && pendingCount > 0 ? (
                <span className="bg-amber-400 text-slate-950 text-[10px] font-black px-1.5 rounded-md tabular-nums" aria-label={`${pendingCount} pending`}>
                  {pendingCount}
                </span>
              ) : id === 'inventory' && stockAlerts > 0 ? (
                <span className="bg-rose-500 text-white text-[10px] font-black px-1.5 rounded-md tabular-nums" aria-label={`${stockAlerts} stock alerts`}>
                  {stockAlerts}
                </span>
              ) : null;
            return (
              <button
                key={id}
                type="button"
                aria-current={active && !orderTarget ? 'page' : undefined}
                onClick={() => {
                  setActiveTab(id);
                  if (orderTarget) handleBackFromDetail();
                }}
                className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
                  active && !orderTarget ? 'bg-slate-950 text-white shadow-xs' : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                <span>{label}</span>
                {badge}
              </button>
            );
          })}
        </nav>
      </header>

      {sync.health === 'failing' && (
        <div role="status" className="bg-amber-50 border-b border-amber-200 text-amber-900 text-xs font-bold">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-2">
            Live updates are interrupted — figures may be out of date. Retrying automatically.
          </div>
        </div>
      )}

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 sm:py-8 flex-1 w-full">
        {orderTarget ? (
          <AdminOrderDetailPage target={orderTarget} onBack={handleBackFromDetail} />
        ) : (
          <>
            {activeTab === 'dashboard' && (
              <AdminDashboardTab
                onOpenOrder={openOrder}
                onNavigate={setActiveTab}
                onShowOrders={showOrders}
                onShowInventoryProduct={showInventoryProduct}
              />
            )}
            {activeTab === 'orders' && (
              <AdminOrdersTab status={orderStatus} onStatusChange={setOrderStatus} highlighted={highlighted} onOpenOrder={openOrder} />
            )}
            {activeTab === 'products' && <AdminProductsTab />}
            {activeTab === 'payments' && <AdminPaymentsTab onOpenOrder={openOrder} />}
            {activeTab === 'inventory' && (
              <AdminInventoryTab
                focusProductId={inventoryFocus}
                onFocusHandled={clearInventoryFocus}
                onOpenOrderNumber={openOrderNumber}
              />
            )}
          </>
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page (auth gate)
// ---------------------------------------------------------------------------

export const AdminPage: React.FC<AdminPageProps> = ({ onBackToStore }) => {
  const { status, sessionEndedReason, clearSessionEndedReason } = useAdminAuth();
  const queryClient = useQueryClient();

  // Never keep another session's admin data in the cache after sign-out.
  useEffect(() => {
    if (status === 'unauthenticated') queryClient.removeQueries({ queryKey: adminKeys.all });
  }, [status, queryClient]);

  if (status === 'loading') {
    return (
      <div className="min-h-screen bg-slate-900 flex items-center justify-center p-4 sm:p-6 text-slate-100">
        <div role="status" aria-live="polite" className="flex flex-col items-center gap-3 text-center">
          <Loader2 className="w-7 h-7 text-amber-400 animate-spin" />
          <p className="text-xs font-bold uppercase tracking-widest text-slate-300">Verifying session…</p>
        </div>
      </div>
    );
  }

  if (status !== 'authenticated') {
    return (
      <div className="min-h-screen bg-slate-900 flex items-center justify-center p-4 sm:p-6 text-slate-100">
        <div className="w-full max-w-md bg-white text-slate-900 rounded-3xl p-8 sm:p-10 shadow-2xl border border-slate-200 animate-in fade-in zoom-in-95 duration-200">
          <div className="flex flex-col items-center text-center mb-8">
            <BrandMark size="lg" />
            <h1 className="text-2xl font-black tracking-tight text-slate-950 font-heading">ZHI-MART ADMIN</h1>
            <p className="text-xs font-bold text-amber-600 uppercase tracking-widest mt-1">Operations Console</p>
            <p className="text-xs text-slate-500 mt-2 max-w-xs">
              Restricted portal for orders, dispatch, payments, catalogue and inventory.
            </p>
          </div>

          {sessionEndedReason && (
            <div
              role="status"
              className="mb-6 p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs font-bold flex items-start gap-2"
            >
              <Clock className="w-4 h-4 shrink-0 text-amber-600 mt-px" />
              <span className="flex-1">{sessionEndedReason}</span>
              <button
                type="button"
                onClick={clearSessionEndedReason}
                aria-label="Dismiss message"
                className="p-0.5 rounded text-amber-700 hover:text-amber-950 hover:bg-amber-100 shrink-0"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          )}

          <AdminLoginForm />

          <div className="mt-5 pt-5 border-t border-slate-100 flex flex-col gap-2.5">
            <button
              type="button"
              onClick={onBackToStore}
              className="w-full py-2 text-xs font-bold text-slate-500 hover:text-slate-900 transition-colors flex items-center justify-center gap-1.5"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
              <span>Back to Storefront</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <ToastProvider>
      <AdminConsole onBackToStore={onBackToStore} />
    </ToastProvider>
  );
};

export default AdminPage;
