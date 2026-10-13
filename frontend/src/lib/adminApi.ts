/**
 * Typed admin API over the auth context's `authFetch` (Bearer token + refresh-on-401).
 * Paths are relative to `/api/v1`. See `backend/API.md` → "Admin endpoints".
 */
import { apiRequest, type ApiRequestOptions } from './api';
import type {
  AdminProduct,
  DashboardResponse,
  InventoryResponse,
  Order,
  OrderCancelInput,
  OrderListFilters,
  OrderListResponse,
  OrderPaymentInput,
  OrderRefundInput,
  PaymentEventsResponse,
  OrderShippingInput,
  OrderStatusInput,
  ProductInput,
  StockAdjustInput,
  StockMovement,
  StorefrontConfig,
  SyncResponse,
} from '../types/admin';

export type AuthFetch = <T = unknown>(
  path: string,
  init?: Omit<ApiRequestOptions, 'accessToken'>
) => Promise<T>;

const enc = encodeURIComponent;

function buildQuery(params: Record<string, string | number | boolean | null | undefined>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue;
    search.set(key, String(value));
  }
  const qs = search.toString();
  return qs ? `?${qs}` : '';
}

export function createAdminApi(authFetch: AuthFetch) {
  return {
    // ---- Dashboard & sync ----
    getDashboard: (signal?: AbortSignal) => authFetch<DashboardResponse>('/admin/dashboard', { signal }),

    getSync: (signal?: AbortSignal) =>
      authFetch<SyncResponse>('/admin/sync', { signal, timeoutMs: 10_000 }),

    // ---- Products ----
    listProducts: async (includeArchived: boolean, signal?: AbortSignal) =>
      (
        await authFetch<{ products: AdminProduct[] }>(
          `/admin/products${buildQuery({ includeArchived })}`,
          { signal }
        )
      ).products,

    createProduct: (input: ProductInput) =>
      authFetch<AdminProduct>('/admin/products', { method: 'POST', body: input }),

    updateProduct: (id: string, input: ProductInput) =>
      authFetch<AdminProduct>(`/admin/products/${enc(id)}`, { method: 'PUT', body: input }),

    archiveProduct: (id: string) =>
      authFetch<void>(`/admin/products/${enc(id)}`, { method: 'DELETE' }),

    restoreProduct: (id: string) =>
      authFetch<AdminProduct>(`/admin/products/${enc(id)}/restore`, { method: 'POST' }),

    // ---- Inventory ----
    getInventory: (signal?: AbortSignal) => authFetch<InventoryResponse>('/admin/inventory', { signal }),

    adjustStock: (productId: string, input: StockAdjustInput) =>
      authFetch<AdminProduct>(`/admin/inventory/${enc(productId)}/adjust`, {
        method: 'POST',
        body: input,
      }),

    listMovements: async (productId: string, limit = 50, signal?: AbortSignal) =>
      (
        await authFetch<{ movements: StockMovement[] }>(
          `/admin/inventory/${enc(productId)}/movements${buildQuery({ limit })}`,
          { signal }
        )
      ).movements,

    // ---- Orders ----
    listOrders: (filters: OrderListFilters & { cursor?: string | null }, signal?: AbortSignal) =>
      authFetch<OrderListResponse>(
        `/admin/orders${buildQuery({
          status: filters.status,
          paymentStatus: filters.paymentStatus,
          includeUnpaid: filters.includeUnpaid ? true : undefined,
          q: filters.q?.trim(),
          limit: filters.limit,
          cursor: filters.cursor,
        })}`,
        { signal }
      ),

    getOrder: (id: string, signal?: AbortSignal) => authFetch<Order>(`/admin/orders/${enc(id)}`, { signal }),

    updateOrderStatus: (id: string, input: OrderStatusInput) =>
      authFetch<Order>(`/admin/orders/${enc(id)}/status`, { method: 'POST', body: input }),

    updateOrderShipping: (id: string, input: OrderShippingInput) =>
      authFetch<Order>(`/admin/orders/${enc(id)}/shipping`, { method: 'PUT', body: input }),

    cancelOrder: (id: string, input: OrderCancelInput) =>
      authFetch<Order>(`/admin/orders/${enc(id)}/cancel`, { method: 'POST', body: input }),

    recordPayment: (id: string, input: OrderPaymentInput) =>
      authFetch<Order>(`/admin/orders/${enc(id)}/payment`, { method: 'POST', body: input }),

    /** Razorpay refund (full when `amount` is null). Longer timeout: the server calls Razorpay. */
    refundOrder: (id: string, input: OrderRefundInput) =>
      authFetch<Order>(`/admin/orders/${enc(id)}/refund`, { method: 'POST', body: input, timeoutMs: 45_000 }),

    getPaymentEvents: (id: string, signal?: AbortSignal) =>
      authFetch<PaymentEventsResponse>(`/admin/orders/${enc(id)}/payment-events`, { signal }),
  };
}

export type AdminApi = ReturnType<typeof createAdminApi>;

/** Public storefront config (no auth). */
export function getStorefrontConfig(signal?: AbortSignal) {
  return apiRequest<StorefrontConfig>('/storefront/config', { signal });
}
