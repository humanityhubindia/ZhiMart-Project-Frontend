import { useMemo } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Catalog, fetchCatalog, fetchStorefrontConfig, StorefrontConfig } from '../lib/catalogApi';
import { isApiError } from '../lib/api';
import { DEFAULT_PRICING, PricingRules } from '../lib/money';
import { CrackerProduct } from '../types';

/** Shared react-query key for the public catalog. Invalidate it after placing an order. */
export const PRODUCTS_QUERY_KEY = ['products'] as const;

/** Retry transient failures (network, timeouts, 5xx, 429) a few times; never 4xx. */
function shouldRetry(failureCount: number, error: unknown): boolean {
  if (failureCount >= 3) return false;
  if (!isApiError(error)) return true;
  return (
    error.status === 0 ||
    error.status >= 500 ||
    error.code === 'too_many_requests' ||
    error.code === 'service_unavailable'
  );
}

function retryDelay(attempt: number, error: unknown): number {
  if (isApiError(error) && typeof error.retryAfter === 'number') {
    return Math.min(30_000, Math.max(1_000, error.retryAfter * 1000));
  }
  return Math.min(10_000, 1_000 * 2 ** attempt);
}

const catalogQueryOptions = {
  queryKey: PRODUCTS_QUERY_KEY,
  queryFn: ({ signal }: { signal: AbortSignal }) => fetchCatalog(signal),
  staleTime: 10_000,
  refetchOnWindowFocus: true,
  refetchOnReconnect: true,
  retry: shouldRetry,
  retryDelay,
} as const;

const selectProducts = (catalog: Catalog) => catalog.products;
const selectTestMode = (catalog: Catalog) => catalog.testMode;
const selectPricing = (catalog: Catalog) => catalog.pricing;

/** The live public catalog (active products only, ordered by the backend). */
export function useProducts() {
  return useQuery<Catalog, Error, CrackerProduct[]>({ ...catalogQueryOptions, select: selectProducts });
}

export const STOREFRONT_CONFIG_QUERY_KEY = ['storefront-config'] as const;

/** `GET /storefront/config` (test mode flag, payment gateway + Cashfree mode, store name). */
export function useStorefrontConfig() {
  return useQuery<StorefrontConfig>({
    queryKey: STOREFRONT_CONFIG_QUERY_KEY,
    queryFn: ({ signal }) => fetchStorefrontConfig(signal),
    staleTime: 60_000,
    refetchOnWindowFocus: true,
    retry: shouldRetry,
    retryDelay,
  });
}

/**
 * Whether checkout takes online (Cashfree) payment. `undefined` while the config is loading or
 * unavailable — show neutral copy then, and let the order's own status decide what happens next.
 */
export function useOnlinePaymentsEnabled(): boolean | undefined {
  return useStorefrontConfig().data?.onlinePaymentsEnabled;
}

/**
 * True when the server charges ₹1 per unit (payment test mode). Uses the storefront config,
 * falling back to the `testMode` flag on `GET /products`.
 */
export function useTestMode(): boolean {
  const config = useStorefrontConfig();
  const catalog = useQuery<Catalog, Error, boolean>({ ...catalogQueryOptions, select: selectTestMode });
  return config.data?.testMode === true || catalog.data === true;
}

/**
 * Two-tier pricing rules from `GET /products` (defaults until the catalog loads). Pass them to
 * `computeTotals(items, rules)`.
 */
export function usePricingRules(): PricingRules {
  const catalog = useQuery<Catalog, Error, PricingRules>({ ...catalogQueryOptions, select: selectPricing });
  const config = useStorefrontConfig();
  return catalog.data ?? config.data?.pricing ?? DEFAULT_PRICING;
}

/** Refetch the catalog (e.g. after an order or a stock conflict). */
export function useInvalidateProducts() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: PRODUCTS_QUERY_KEY });
}

export function findProductById(
  products: CrackerProduct[] | undefined,
  id: string | null | undefined
): CrackerProduct | undefined {
  if (!products || !id) return undefined;
  return products.find((p) => p.id === id);
}

/** `Map<id, product>` over the given list, memoised on the list reference. */
export function useProductsById(products: CrackerProduct[] | undefined) {
  return useMemo(() => new Map((products ?? []).map((p) => [p.id, p] as const)), [products]);
}

/** Products that can currently be ordered. */
export const inStockOnly = (products: CrackerProduct[]) =>
  products.filter((p) => p.inStock && p.availableQuantity > 0);

/** Distinct values of a product field with counts, in first-seen (backend sort) order. */
export function countBy(
  products: CrackerProduct[],
  key: 'category' | 'brand'
): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const p of products) {
    const value = p[key];
    if (!value) continue;
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  return Array.from(counts, ([name, count]) => ({ name, count }));
}
