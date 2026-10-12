import { useQuery } from '@tanstack/react-query';
import { ZHI_MART_PRODUCTS } from '../data/crackers';
import { CrackerProduct, FilterTab } from '../types';

interface FilterOptions {
  tab?: FilterTab;
  searchQuery?: string;
  categoryFilter?: string;
  brandFilter?: string;
}

export function useCrackers(filters?: FilterOptions) {
  return useQuery({
    queryKey: ['zhi-mart-products', filters],
    queryFn: async (): Promise<CrackerProduct[]> => {
      // Light simulated delay
      await new Promise((res) => setTimeout(res, 80));

      let result = [...ZHI_MART_PRODUCTS];

      if (filters?.tab) {
        if (filters.tab === 'popular') {
          result = result.filter((p) => p.isPopular);
        } else if (filters.tab === 'bestselling') {
          result = result.filter((p) => p.isBestSelling);
        } else if (filters.tab === 'newarrivals') {
          result = result.filter((p) => p.isNewArrival);
        } else if (filters.tab === 'party') {
          result = result.filter((p) => p.isParty);
        }
      }

      if (filters?.categoryFilter && filters.categoryFilter !== 'all') {
        result = result.filter((p) => p.category.toLowerCase().includes(filters.categoryFilter!.toLowerCase()));
      }

      if (filters?.brandFilter && filters.brandFilter !== 'all') {
        result = result.filter((p) => p.brand.toLowerCase().includes(filters.brandFilter!.toLowerCase()));
      }

      if (filters?.searchQuery) {
        const q = filters.searchQuery.toLowerCase().trim();
        result = result.filter(
          (p) =>
            p.name.toLowerCase().includes(q) ||
            p.category.toLowerCase().includes(q) ||
            p.brand.toLowerCase().includes(q)
        );
      }

      return result;
    },
  });
}
