import { useEffect, useRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import type { SyncResponse } from '../../types/admin';
import { adminKeys, invalidateCatalogData, invalidateOrderData, useAdminApi } from './adminQueries';

export const SYNC_INTERVAL_MS = 5_000;

export type SyncHealth = 'connecting' | 'live' | 'failing';

export interface AdminSyncState {
  health: SyncHealth;
  data: SyncResponse | undefined;
  lastSyncedAt: number | null;
  error: unknown;
}

/**
 * Polls `GET /admin/sync` every 5 s while the tab is visible (react-query pauses
 * interval refetches for hidden documents) and invalidates only the queries whose
 * change marker moved. Calls `onNewOrder` when `latestOrderNumber` changes.
 */
export function useAdminSync(onNewOrder?: (orderNumber: string) => void): AdminSyncState {
  const api = useAdminApi();
  const qc = useQueryClient();
  const prevRef = useRef<SyncResponse | null>(null);
  const onNewOrderRef = useRef(onNewOrder);
  onNewOrderRef.current = onNewOrder;

  const query = useQuery({
    queryKey: adminKeys.sync(),
    queryFn: ({ signal }) => api.getSync(signal),
    refetchInterval: SYNC_INTERVAL_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });

  const { data, dataUpdatedAt } = query;

  useEffect(() => {
    if (!data) return;
    const prev = prevRef.current;
    prevRef.current = data;
    if (!prev) return; // first snapshot is the baseline

    if (prev.ordersVersion !== data.ordersVersion) {
      void invalidateOrderData(qc);
      // New/cancelled orders move stock too.
      void qc.invalidateQueries({ queryKey: adminKeys.inventory() });
    }
    if (prev.productsVersion !== data.productsVersion) {
      void invalidateCatalogData(qc);
    }
    if (data.latestOrderNumber && data.latestOrderNumber !== prev.latestOrderNumber) {
      onNewOrderRef.current?.(data.latestOrderNumber);
    }
    // dataUpdatedAt changes on every successful poll even when `data` is structurally equal.
  }, [data, dataUpdatedAt, qc]);

  const health: SyncHealth = query.isError ? 'failing' : data ? 'live' : 'connecting';

  return {
    health,
    data,
    lastSyncedAt: dataUpdatedAt || null,
    error: query.error,
  };
}
