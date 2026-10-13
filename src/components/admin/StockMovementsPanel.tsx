import React from 'react';
import { History, PackageCheck, Pencil } from 'lucide-react';
import { useInventory, useStockMovements } from '../../hooks/admin/adminQueries';
import { formatCount, formatDateTime, movementReasonLabel } from './format';
import { Button, Drawer, EmptyState, ErrorCard, InventoryStatusBadge, ProductImage, Skeleton } from './ui';

const REASON_TONE: Record<string, string> = {
  initial: 'bg-slate-100 text-slate-700',
  order_placed: 'bg-blue-50 text-blue-800',
  order_cancelled: 'bg-purple-50 text-purple-800',
  manual_set: 'bg-amber-50 text-amber-800',
  manual_adjust: 'bg-rose-50 text-rose-700',
  restock: 'bg-emerald-50 text-emerald-800',
};

export function StockMovementsPanel({
  productId,
  onClose,
  onOpenOrderNumber,
  onAdjust,
  onEditProduct,
}: {
  productId: string | null;
  onClose: () => void;
  onOpenOrderNumber: (orderNumber: string) => void;
  onAdjust: (productId: string) => void;
  onEditProduct: (productId: string) => void;
}) {
  const movements = useStockMovements(productId);
  const inventory = useInventory();
  const item = inventory.data?.items.find((i) => i.productId === productId);

  return (
    <Drawer
      open={productId !== null}
      onClose={onClose}
      label="Stock history"
      title={
        <div className="flex items-center gap-3">
          {item && <ProductImage src={item.image} alt={item.name} className="w-10 h-10" />}
          <div className="min-w-0">
            <h2 className="text-base font-black text-slate-950 truncate flex items-center gap-1.5">
              <History className="w-4 h-4 text-slate-400" /> Stock history
            </h2>
            <p className="text-xs text-slate-500 truncate">
              {item?.name ?? productId} <span className="font-mono">· {productId}</span>
            </p>
          </div>
        </div>
      }
    >
      <div className="p-4 sm:p-6 space-y-4">
        {item && (
          <div className="bg-white rounded-2xl border border-slate-200 p-4 flex flex-wrap items-center gap-4">
            <div>
              <p className="text-[11px] font-bold text-slate-500 uppercase tracking-wider">Current stock</p>
              <p className="text-2xl font-black text-slate-950 tabular-nums">{formatCount(item.stock)}</p>
            </div>
            <InventoryStatusBadge status={item.status} />
            <div className="flex gap-2 ml-auto">
              <Button size="sm" variant="primary" icon={PackageCheck} onClick={() => onAdjust(item.productId)}>
                Adjust stock
              </Button>
              <Button size="sm" icon={Pencil} onClick={() => onEditProduct(item.productId)}>
                Threshold / bay
              </Button>
            </div>
          </div>
        )}

        {movements.isPending ? (
          <div className="space-y-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <Skeleton key={i} className="h-14 rounded-2xl" />
            ))}
          </div>
        ) : movements.isError ? (
          <ErrorCard compact title="Could not load stock history" error={movements.error} onRetry={() => void movements.refetch()} retrying={movements.isFetching} />
        ) : movements.data.length === 0 ? (
          <EmptyState icon={History} title="No stock movements yet" />
        ) : (
          <div className="bg-white rounded-2xl border border-slate-200 overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-xs">
                <thead className="bg-slate-50 text-[10px] uppercase tracking-wider text-slate-500 text-left">
                  <tr>
                    <th scope="col" className="px-3 py-2.5 font-extrabold">When (IST)</th>
                    <th scope="col" className="px-3 py-2.5 font-extrabold text-right">Change</th>
                    <th scope="col" className="px-3 py-2.5 font-extrabold text-right">After</th>
                    <th scope="col" className="px-3 py-2.5 font-extrabold">Reason</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {movements.data.map((m) => (
                    <tr key={m.id} className="align-top">
                      <td className="px-3 py-2.5 text-slate-600 whitespace-nowrap">{formatDateTime(m.createdAt)}</td>
                      <td
                        className={`px-3 py-2.5 text-right font-black tabular-nums whitespace-nowrap ${
                          m.delta > 0 ? 'text-emerald-700' : 'text-rose-600'
                        }`}
                      >
                        {m.delta > 0 ? '+' : '−'}
                        {formatCount(Math.abs(m.delta))}
                      </td>
                      <td className="px-3 py-2.5 text-right font-bold tabular-nums text-slate-900">{formatCount(m.stockAfter)}</td>
                      <td className="px-3 py-2.5">
                        <span className={`inline-block px-1.5 py-0.5 rounded-md text-[10px] font-extrabold ${REASON_TONE[m.reason] ?? 'bg-slate-100 text-slate-700'}`}>
                          {movementReasonLabel(m.reason)}
                        </span>
                        {m.orderNumber && (
                          <button
                            type="button"
                            onClick={() => onOpenOrderNumber(m.orderNumber as string)}
                            className="ml-1.5 font-mono font-bold text-blue-700 hover:underline"
                          >
                            {m.orderNumber}
                          </button>
                        )}
                        {m.adminEmail && <p className="text-[10px] text-slate-400 mt-0.5">by {m.adminEmail}</p>}
                        {m.note && <p className="text-[11px] text-slate-600 mt-0.5 break-words">“{m.note}”</p>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="px-3 py-2 text-[10px] text-slate-400 border-t border-slate-100">
              Latest {movements.data.length} movements.
            </p>
          </div>
        )}
      </div>
    </Drawer>
  );
}
