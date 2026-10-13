import { useMemo } from 'react';
import {
  keepPreviousData,
  useInfiniteQuery,
  useQuery,
  type QueryClient,
} from '@tanstack/react-query';
import { useAdminAuth } from '../../auth/AdminAuthContext';
import { createAdminApi, getStorefrontConfig, type AdminApi } from '../../lib/adminApi';
import { isApiError } from '../../lib/api';
import type { Order, OrderListFilters } from '../../types/admin';

/** Query-key factory. Everything admin lives under `['admin']`. */
export const adminKeys = {
  all: ['admin'] as const,
  sync: () => ['admin', 'sync'] as const,
  dashboard: () => ['admin', 'dashboard'] as const,
  orders: () => ['admin', 'orders'] as const,
  orderList: (filters: OrderListFilters) => ['admin', 'orders', 'list', filters] as const,
  orderDetail: (id: string) => ['admin', 'orders', 'detail', id] as const,
  orderLookup: (orderNumber: string) => ['admin', 'orders', 'lookup', orderNumber] as const,
  paymentEvents: (id: string) => ['admin', 'orders', 'payment-events', id] as const,
  storefrontConfig: () => ['admin', 'storefront-config'] as const,
  products: () => ['admin', 'products'] as const,
  productList: (includeArchived: boolean) => ['admin', 'products', { includeArchived }] as const,
  inventory: () => ['admin', 'inventory'] as const,
  movements: (productId: string) => ['admin', 'inventory', 'movements', productId] as const,
};

/** Storefront catalogue key (owned by the storefront hooks). */
export const STOREFRONT_PRODUCTS_KEY = ['products'] as const;

/** Admin data is kept fresh by `useAdminSync`, so a short stale time is enough. */
const ADMIN_STALE_MS = 15_000;

/** Don't retry definitive client errors (4xx); retry transient ones twice. */
function retryTransient(failureCount: number, error: unknown): boolean {
  if (isApiError(error) && error.status >= 400 && error.status < 500 && error.status !== 429) return false;
  return failureCount < 2;
}

export function useAdminApi(): AdminApi {
  const { authFetch } = useAdminAuth();
  return useMemo(() => createAdminApi(authFetch), [authFetch]);
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

export function useDashboard() {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.dashboard(),
    queryFn: ({ signal }) => api.getDashboard(signal),
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
  });
}

export const ORDERS_PAGE_SIZE = 30;

export function useAdminOrders(filters: OrderListFilters) {
  const api = useAdminApi();
  const normalized: OrderListFilters = {
    status: filters.status ?? null,
    paymentStatus: filters.paymentStatus ?? null,
    includeUnpaid: filters.includeUnpaid ?? false,
    q: filters.q?.trim() || null,
    limit: filters.limit ?? ORDERS_PAGE_SIZE,
  };
  return useInfiniteQuery({
    queryKey: adminKeys.orderList(normalized),
    queryFn: ({ pageParam, signal }) => api.listOrders({ ...normalized, cursor: pageParam }, signal),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.nextCursor ?? undefined,
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
    placeholderData: keepPreviousData,
  });
}

export function useAdminOrder(id: string | null, seed?: Order | null) {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.orderDetail(id ?? ''),
    queryFn: ({ signal }) => api.getOrder(id as string, signal),
    enabled: !!id,
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
    placeholderData: seed && seed.id === id ? seed : undefined,
  });
}

/** Resolve an order number (e.g. from a stock movement) to an order. */
export function useOrderLookup(orderNumber: string | null) {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.orderLookup(orderNumber ?? ''),
    // `GET /admin/orders/{ref}` accepts an order number as well as the UUID.
    queryFn: async ({ signal }) => {
      try {
        return await api.getOrder(orderNumber as string, signal);
      } catch (err) {
        if (isApiError(err) && err.code === 'not_found') return null;
        throw err;
      }
    },
    enabled: !!orderNumber,
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
  });
}

export function usePaymentEvents(orderId: string | null) {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.paymentEvents(orderId ?? ''),
    queryFn: ({ signal }) => api.getPaymentEvents(orderId as string, signal),
    enabled: !!orderId,
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
  });
}

/** Public `GET /storefront/config` — drives the payment test-mode banner. */
export function useStorefrontConfig() {
  return useQuery({
    queryKey: adminKeys.storefrontConfig(),
    queryFn: ({ signal }) => getStorefrontConfig(signal),
    staleTime: 60_000,
    refetchInterval: 5 * 60_000,
    refetchIntervalInBackground: false,
    retry: retryTransient,
  });
}

export function useAdminProducts(includeArchived: boolean) {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.productList(includeArchived),
    queryFn: ({ signal }) => api.listProducts(includeArchived, signal),
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
    placeholderData: keepPreviousData,
  });
}

export function useInventory() {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.inventory(),
    queryFn: ({ signal }) => api.getInventory(signal),
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
  });
}

export function useStockMovements(productId: string | null, limit = 50) {
  const api = useAdminApi();
  return useQuery({
    queryKey: adminKeys.movements(productId ?? ''),
    queryFn: ({ signal }) => api.listMovements(productId as string, limit, signal),
    enabled: !!productId,
    staleTime: ADMIN_STALE_MS,
    retry: retryTransient,
  });
}

// ---------------------------------------------------------------------------
// Invalidation helpers (shared by mutations and the live-sync poller)
// ---------------------------------------------------------------------------

export function invalidateOrderData(qc: QueryClient) {
  return Promise.all([
    qc.invalidateQueries({ queryKey: adminKeys.orders() }),
    qc.invalidateQueries({ queryKey: adminKeys.dashboard() }),
  ]);
}

export function invalidateCatalogData(qc: QueryClient) {
  return Promise.all([
    qc.invalidateQueries({ queryKey: adminKeys.products() }),
    qc.invalidateQueries({ queryKey: adminKeys.inventory() }),
    qc.invalidateQueries({ queryKey: adminKeys.dashboard() }),
    qc.invalidateQueries({ queryKey: STOREFRONT_PRODUCTS_KEY }),
  ]);
}
