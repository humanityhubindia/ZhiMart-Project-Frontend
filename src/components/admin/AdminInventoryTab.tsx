import React, { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Boxes, CheckCircle2, History, PackageCheck, PackageX, Pencil, Search, Warehouse, X } from 'lucide-react';
import { useAdminProducts, useInventory } from '../../hooks/admin/adminQueries';
import type { InventoryStatus } from '../../types/admin';
import { formatCount, formatDateTime, INVENTORY_STATUS_META } from './format';
import { ProductFormModal } from './ProductFormModal';
import { uniqueSorted } from './AdminProductsTab';
import { StockAdjustModal } from './StockAdjustModal';
import { StockMovementsPanel } from './StockMovementsPanel';
import {
  ArchivedBadge,
  Button,
  EmptyState,
  ErrorCard,
  FilterChip,
  inputClass,
  InventoryStatusBadge,
  KpiCard,
  ProductImage,
  Skeleton,
  Spinner,
} from './ui';

/** Loads the full product (PUT needs every field) and opens the edit form. */
function InventoryProductEditor({ productId, onClose }: { productId: string; onClose: () => void }) {
  const products = useAdminProducts(true);
  const product = products.data?.find((p) => p.id === productId) ?? null;
  const list = products.data ?? [];
  const categories = useMemo(() => uniqueSorted(list.map((p) => p.category)), [list]);
  const brands = useMemo(() => uniqueSorted(list.map((p) => p.brand)), [list]);

  if (products.isPending) {
    return (
      <div className="fixed inset-0 z-[60] bg-slate-950/40 flex items-center justify-center">
        <div className="bg-white rounded-2xl px-5 py-4 shadow-xl">
          <Spinner label="Loading product…" />
        </div>
      </div>
    );
  }
  if (!product) {
    return (
      <div className="fixed inset-0 z-[60] bg-slate-950/40 flex items-center justify-center p-4" onClick={onClose}>
        <div className="max-w-md w-full" onClick={(e) => e.stopPropagation()}>
          <ErrorCard
            compact
            title="Could not open this product"
            error={products.error ?? new Error('Product not found.')}
            onRetry={() => void products.refetch()}
          />
          <div className="text-center mt-2">
            <Button variant="secondary" onClick={onClose}>
              Close
            </Button>
          </div>
        </div>
      </div>
    );
  }
  return <ProductFormModal open product={product} categories={categories} brands={brands} onClose={onClose} />;
}

export function AdminInventoryTab({
  focusProductId,
  onFocusHandled,
  onOpenOrderNumber,
}: {
  focusProductId: string | null;
  onFocusHandled: () => void;
  onOpenOrderNumber: (orderNumber: string) => void;
}) {
  const inventory = useInventory();
  const [search, setSearch] = useState('');
  const [status, setStatus] = useState<InventoryStatus | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [adjustId, setAdjustId] = useState<string | null>(null);
  const [historyId, setHistoryId] = useState<string | null>(null);
  const [editId, setEditId] = useState<string | null>(null);
  const [flashId, setFlashId] = useState<string | null>(null);
  const rowRefs = useRef(new Map<string, HTMLTableRowElement>());

  const items = inventory.data?.items ?? [];
  const summary = inventory.data?.summary;

  // Dashboard → "show this product": clear filters, then scroll to and flash the row.
  useEffect(() => {
    if (!focusProductId || !inventory.data) return;
    setSearch('');
    setStatus(null);
    const target = inventory.data.items.find((i) => i.productId === focusProductId);
    if (target && !target.isActive) setShowArchived(true);
    setFlashId(focusProductId);
    onFocusHandled();
  }, [focusProductId, inventory.data, onFocusHandled]);

  useEffect(() => {
    if (!flashId) return;
    const scroll = setTimeout(() => {
      rowRefs.current.get(flashId)?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }, 50);
    const clear = setTimeout(() => setFlashId(null), 4000);
    return () => {
      clearTimeout(scroll);
      clearTimeout(clear);
    };
  }, [flashId]);

  const counts = useMemo(() => {
    const visible = items.filter((i) => showArchived || i.isActive);
    return {
      all: visible.length,
      in_stock: visible.filter((i) => i.status === 'in_stock').length,
      low_stock: visible.filter((i) => i.status === 'low_stock').length,
      out_of_stock: visible.filter((i) => i.status === 'out_of_stock').length,
    };
  }, [items, showArchived]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return items
      .filter((i) => showArchived || i.isActive)
      .filter((i) => !status || i.status === status)
      .filter(
        (i) =>
          !q ||
          i.name.toLowerCase().includes(q) ||
          i.productId.toLowerCase().includes(q) ||
          i.category.toLowerCase().includes(q) ||
          (i.bayLocation ?? '').toLowerCase().includes(q)
      );
  }, [items, search, status, showArchived]);

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">Inventory</h2>
        <p className="text-xs text-slate-500 font-semibold mt-0.5">
          Live stock from the database. Orders reserve stock instantly; cancellations return it. Every change is logged.
        </p>
      </div>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        {inventory.isPending ? (
          Array.from({ length: 4 }).map((_, i) => <Skeleton key={i} className="h-[124px] rounded-3xl" />)
        ) : summary ? (
          <>
            <KpiCard label="Units in stock" value={formatCount(summary.totalUnits)} icon={Boxes} tone="slate" />
            <KpiCard label="In stock" value={formatCount(summary.inStock)} icon={CheckCircle2} tone="emerald" hint="products" onClick={() => setStatus('in_stock')} />
            <KpiCard label="Low stock" value={formatCount(summary.lowStock)} icon={AlertTriangle} tone="amber" hint="products" onClick={() => setStatus('low_stock')} />
            <KpiCard label="Out of stock" value={formatCount(summary.outOfStock)} icon={PackageX} tone="rose" hint="products" onClick={() => setStatus('out_of_stock')} />
          </>
        ) : null}
      </div>

      <div className="flex flex-col lg:flex-row gap-2 lg:items-center justify-between">
        <div className="flex flex-wrap gap-2" role="group" aria-label="Filter by stock status">
          <FilterChip active={status === null} onClick={() => setStatus(null)} count={inventory.data ? counts.all : null}>
            All
          </FilterChip>
          {(['in_stock', 'low_stock', 'out_of_stock'] as InventoryStatus[]).map((s) => (
            <FilterChip key={s} active={status === s} onClick={() => setStatus(s)} count={inventory.data ? counts[s] : null}>
              {INVENTORY_STATUS_META[s].label}
            </FilterChip>
          ))}
        </div>
        <div className="flex flex-col sm:flex-row gap-2">
          <div className="relative sm:w-72">
            <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" aria-hidden />
            <label htmlFor="inventory-search" className="sr-only">
              Search inventory
            </label>
            <input
              id="inventory-search"
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Name, ID, category or bay"
              className={`${inputClass} pl-10`}
            />
          </div>
          <label className="inline-flex items-center gap-2 text-xs font-bold text-slate-700 bg-white border border-slate-200 rounded-xl px-3.5 py-2.5 cursor-pointer whitespace-nowrap">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(e) => setShowArchived(e.target.checked)}
              className="accent-amber-500 w-4 h-4"
            />
            Show archived
          </label>
        </div>
      </div>

      {inventory.isPending ? (
        <div className="space-y-2" aria-busy="true">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-16 rounded-2xl" />
          ))}
        </div>
      ) : inventory.isError && !inventory.data ? (
        <ErrorCard title="Could not load inventory" error={inventory.error} onRetry={() => void inventory.refetch()} retrying={inventory.isFetching} />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={Warehouse}
          title={items.length === 0 ? 'No products in inventory' : 'Nothing matches'}
          description={items.length === 0 ? 'Add products from the Products tab.' : 'Try another filter or search.'}
          action={
            items.length > 0 ? (
              <Button
                icon={X}
                onClick={() => {
                  setSearch('');
                  setStatus(null);
                }}
              >
                Clear filters
              </Button>
            ) : undefined
          }
        />
      ) : (
        <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2.5 border-b border-slate-100 text-[11px] font-bold text-slate-500">
            <span>{filtered.length} products</span>
            {inventory.isFetching && <Spinner className="w-3.5 h-3.5" label="Refreshing…" />}
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="bg-slate-50 text-[10px] uppercase tracking-wider text-slate-500 text-left">
                <tr>
                  <th scope="col" className="px-4 py-3 font-extrabold">Product</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Bay</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Stock</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Low at</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Status</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Updated</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {filtered.map((i) => (
                  <tr
                    key={i.productId}
                    ref={(el) => {
                      if (el) rowRefs.current.set(i.productId, el);
                      else rowRefs.current.delete(i.productId);
                    }}
                    className={`align-middle transition-colors ${
                      flashId === i.productId ? 'bg-amber-50' : !i.isActive ? 'bg-slate-50/80' : 'hover:bg-slate-50/60'
                    }`}
                  >
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-3 min-w-[200px]">
                        <ProductImage src={i.image} alt={i.name} className={`w-10 h-10 ${!i.isActive ? 'opacity-50 grayscale' : ''}`} />
                        <div className="min-w-0">
                          <p className="font-extrabold text-slate-900">{i.name}</p>
                          <p className="text-[10px] text-slate-400">
                            <span className="font-mono">{i.productId}</span> · {i.category}
                          </p>
                          {!i.isActive && (
                            <div className="mt-1">
                              <ArchivedBadge />
                            </div>
                          )}
                        </div>
                      </div>
                    </td>
                    <td className="px-4 py-3 text-slate-600 whitespace-nowrap">{i.bayLocation ?? <span className="text-slate-300">—</span>}</td>
                    <td
                      className={`px-4 py-3 text-right text-base font-black tabular-nums ${
                        i.status === 'out_of_stock' ? 'text-rose-600' : i.status === 'low_stock' ? 'text-amber-700' : 'text-slate-950'
                      }`}
                    >
                      {formatCount(i.stock)}
                    </td>
                    <td className="px-4 py-3 text-right tabular-nums text-slate-500">{formatCount(i.lowStockThreshold)}</td>
                    <td className="px-4 py-3">
                      <InventoryStatusBadge status={i.status} />
                    </td>
                    <td className="px-4 py-3 text-[11px] text-slate-500 whitespace-nowrap">{formatDateTime(i.updatedAt)}</td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1.5">
                        <Button size="sm" variant="primary" icon={PackageCheck} onClick={() => setAdjustId(i.productId)} aria-label={`Adjust stock for ${i.name}`}>
                          Adjust
                        </Button>
                        <Button size="sm" icon={History} onClick={() => setHistoryId(i.productId)} aria-label={`Stock history for ${i.name}`}>
                          History
                        </Button>
                        <Button size="sm" variant="ghost" icon={Pencil} onClick={() => setEditId(i.productId)} aria-label={`Edit threshold and bay for ${i.name}`} title="Edit low-stock threshold, bay & product details">
                          <span className="sr-only">Edit</span>
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="px-4 py-2.5 border-t border-slate-100 text-[11px] text-slate-400">
            Low-stock threshold and bay location are edited with the product (pencil icon).
          </p>
        </div>
      )}

      <StockAdjustModal productId={adjustId} onClose={() => setAdjustId(null)} />
      <StockMovementsPanel
        productId={historyId}
        onClose={() => setHistoryId(null)}
        onOpenOrderNumber={onOpenOrderNumber}
        onAdjust={(id) => setAdjustId(id)}
        onEditProduct={(id) => setEditId(id)}
      />
      {editId && <InventoryProductEditor productId={editId} onClose={() => setEditId(null)} />}
    </div>
  );
}
