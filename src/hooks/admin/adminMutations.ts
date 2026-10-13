import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { isApiError } from '../../lib/api';
import type {
  AdminProduct,
  Order,
  OrderCancelInput,
  OrderPaymentInput,
  OrderRefundInput,
  OrderShippingInput,
  OrderStatusInput,
  ProductInput,
  StockAdjustInput,
} from '../../types/admin';
import {
  adminKeys,
  invalidateCatalogData,
  invalidateOrderData,
  useAdminApi,
} from './adminQueries';

/** Errors that mean "our copy is stale" — refetch so the UI shows the truth. */
function isStaleError(err: unknown): boolean {
  return (
    isApiError(err) &&
    (err.code === 'version_conflict' || err.code === 'invalid_transition' || err.code === 'not_found')
  );
}

function applyOrder(qc: QueryClient, order: Order) {
  qc.setQueryData(adminKeys.orderDetail(order.id), order);
  void invalidateOrderData(qc);
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

function useOrderMutation<TInput>(
  run: (id: string, input: TInput) => Promise<Order>,
  opts: { touchesStock?: boolean } = {}
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: TInput }) => run(id, input),
    onSuccess: (order) => {
      applyOrder(qc, order);
      if (opts.touchesStock) void invalidateCatalogData(qc);
    },
    onError: (err, { id }) => {
      if (isStaleError(err)) {
        void qc.invalidateQueries({ queryKey: adminKeys.orderDetail(id) });
        void invalidateOrderData(qc);
      }
    },
  });
}

export function useUpdateOrderStatus() {
  const api = useAdminApi();
  return useOrderMutation<OrderStatusInput>((id, input) => api.updateOrderStatus(id, input));
}

export function useUpdateOrderShipping() {
  const api = useAdminApi();
  return useOrderMutation<OrderShippingInput>((id, input) => api.updateOrderShipping(id, input));
}

export function useCancelOrder() {
  const api = useAdminApi();
  // Cancelling restocks every line.
  return useOrderMutation<OrderCancelInput>((id, input) => api.cancelOrder(id, input), {
    touchesStock: true,
  });
}

export function useRecordPayment() {
  const api = useAdminApi();
  return useOrderMutation<OrderPaymentInput>((id, input) => api.recordPayment(id, input));
}

export function useRefundOrder() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: OrderRefundInput }) => api.refundOrder(id, input),
    // Payment events live under ['admin','orders'], so this also refreshes the refund history.
    onSuccess: (order) => applyOrder(qc, order),
    onError: (_err, { id }) => {
      // Even a gateway failure may have recorded a (failed/pending) refund row.
      void qc.invalidateQueries({ queryKey: adminKeys.orderDetail(id) });
      void qc.invalidateQueries({ queryKey: adminKeys.paymentEvents(id) });
      void invalidateOrderData(qc);
    },
  });
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

export function useCreateProduct() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: ProductInput) => api.createProduct(input),
    onSuccess: () => void invalidateCatalogData(qc),
  });
}

export function useUpdateProduct() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: ProductInput }) => api.updateProduct(id, input),
    onSuccess: (product) => {
      void invalidateCatalogData(qc);
      void qc.invalidateQueries({ queryKey: adminKeys.movements(product.id) });
    },
    onError: (err) => {
      if (isStaleError(err)) void invalidateCatalogData(qc);
    },
  });
}

export function useArchiveProduct() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.archiveProduct(id),
    onSettled: () => void invalidateCatalogData(qc),
  });
}

export function useRestoreProduct() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.restoreProduct(id),
    onSettled: () => void invalidateCatalogData(qc),
  });
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

export function useAdjustStock() {
  const api = useAdminApi();
  const qc = useQueryClient();
  return useMutation<AdminProduct, unknown, { productId: string; input: StockAdjustInput }>({
    mutationFn: ({ productId, input }) => api.adjustStock(productId, input),
    onSuccess: (_product, { productId }) => {
      void invalidateCatalogData(qc);
      void qc.invalidateQueries({ queryKey: adminKeys.movements(productId) });
    },
    onError: (err) => {
      if (isApiError(err) && (err.code === 'version_conflict' || err.code === 'out_of_stock')) {
        void qc.invalidateQueries({ queryKey: adminKeys.inventory() });
      }
    },
  });
}
