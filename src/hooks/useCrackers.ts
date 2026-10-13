import { useMemo } from 'react';
import { useProducts } from './useProducts';
import { CrackerProduct, FilterTab } from '../types';

export interface CrackerFilterOptions {
  tab?: FilterTab;
  searchQuery?: string;
  categoryFilter?: string;
  brandFilter?: string;
}

export function filterProducts(
  products: CrackerProduct[],
  filters: CrackerFilterOptions = {}
): CrackerProduct[] {
  let result = products;

  switch (filters.tab) {
    case 'popular':
      result = result.filter((p) => p.isPopular);
      break;
    case 'bestselling':
      result = result.filter((p) => p.isBestSelling);
      break;
    case 'newarrivals':
      result = result.filter((p) => p.isNewArrival);
      break;
    case 'party':
      result = result.filter((p) => p.isParty);
      break;
    default:
      break;
  }

  if (filters.categoryFilter && filters.categoryFilter !== 'all') {
    const category = filters.categoryFilter.toLowerCase();
    result = result.filter((p) => p.category.toLowerCase() === category);
  }

  if (filters.brandFilter && filters.brandFilter !== 'all') {
    const brand = filters.brandFilter.toLowerCase();
    result = result.filter((p) => p.brand.toLowerCase() === brand);
  }

  const q = filters.searchQuery?.toLowerCase().trim();
  if (q) {
    result = result.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        (p.tamilName ?? '').toLowerCase().includes(q) ||
        p.category.toLowerCase().includes(q) ||
        p.brand.toLowerCase().includes(q)
    );
  }

  return result;
}

/**
 * Catalog products from the backend, filtered client-side.
 * `products` is the filtered list; `allProducts` the full live catalog.
 */
export function useCrackers(filters?: CrackerFilterOptions) {
  const query = useProducts();
  const { tab, searchQuery, categoryFilter, brandFilter } = filters ?? {};

  const products = useMemo(
    () => filterProducts(query.data ?? [], { tab, searchQuery, categoryFilter, brandFilter }),
    [query.data, tab, searchQuery, categoryFilter, brandFilter]
  );

  return {
    products,
    allProducts: query.data,
    isLoading: query.isPending,
    isError: query.isError,
    isFetching: query.isFetching,
    error: query.error,
    refetch: query.refetch,
  };
}
