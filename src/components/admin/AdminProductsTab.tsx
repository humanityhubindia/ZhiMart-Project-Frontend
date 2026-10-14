import React, { useMemo, useState } from 'react';
import { Archive, ArchiveRestore, Clock, Flame, Pencil, Plus, Search, ShoppingBag, Star, TrendingUp, X } from 'lucide-react';
import { useAdminProducts } from '../../hooks/admin/adminQueries';
import { useArchiveProduct, useRestoreProduct } from '../../hooks/admin/adminMutations';
import type { AdminProduct } from '../../types/admin';
import { usePricingRules } from '../../hooks/useProducts';
import { errorMessage, formatCount, formatINR, standardPriceFor } from './format';
import { ProductFormModal } from './ProductFormModal';
import { useToast } from './Toast';
import {
  ArchivedBadge,
  Button,
  ConfirmDialog,
  EmptyState,
  ErrorCard,
  inputClass,
  ProductImage,
  Skeleton,
  Spinner,
} from './ui';

export function uniqueSorted(values: string[]): string[] {
  return Array.from(new Set(values.filter(Boolean))).sort((a, b) => a.localeCompare(b));
}

function StockCell({ p }: { p: AdminProduct }) {
  const tone = p.stock === 0 ? 'text-rose-600' : p.stock <= p.lowStockThreshold ? 'text-amber-700' : 'text-slate-900';
  const label = p.stock === 0 ? 'Out of stock' : p.stock <= p.lowStockThreshold ? 'Low' : null;
  return (
    <span className="whitespace-nowrap">
      <span className={`font-black tabular-nums ${tone}`}>{formatCount(p.stock)}</span>
      {label && <span className={`block text-[10px] font-extrabold ${tone}`}>{label}</span>}
    </span>
  );
}

/** Standard (below-threshold) price: the server's `standardPrice` when present, else the preview formula. */
function StandardPriceLine({ p, standardDiscountPercent }: { p: AdminProduct; standardDiscountPercent: number }) {
  const fromServer = typeof p.standardPrice === 'number' && p.standardPrice > 0;
  const value = fromServer ? p.standardPrice! : standardPriceFor(p.price, p.originalPrice, standardDiscountPercent);
  if (value === null || !(value > p.price)) return null;
  const pct =
    fromServer && typeof p.standardDiscountPercent === 'number'
      ? p.standardDiscountPercent
      : p.originalPrice > 0
        ? Math.round(((p.originalPrice - value) / p.originalPrice) * 100)
        : 0;
  return (
    <p
      className="text-[11px] text-sky-800 font-bold tabular-nums"
      title={fromServer ? 'Standard price (below the bulk threshold)' : 'Standard price preview (computed from MRP)'}
    >
      Std {formatINR(value)} <span className="font-semibold text-slate-400">· {pct}% off</span>
    </p>
  );
}

function Flags({ p }: { p: AdminProduct }) {
  const flags = [
    p.isPopular && { icon: Star, label: 'Popular' },
    p.isBestSelling && { icon: TrendingUp, label: 'Best selling' },
    p.isNewArrival && { icon: Clock, label: 'New arrival' },
    p.isParty && { icon: Flame, label: 'Party' },
  ].filter(Boolean) as { icon: React.ComponentType<{ className?: string }>; label: string }[];
  if (!flags.length) return <span className="text-slate-300">—</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {flags.map(({ icon: Icon, label }) => (
        <span key={label} title={label} className="inline-flex items-center gap-1 text-[10px] font-bold bg-amber-50 text-amber-800 border border-amber-200 px-1.5 py-0.5 rounded-md">
          <Icon className="w-3 h-3" />
          {label}
        </span>
      ))}
    </span>
  );
}

export function AdminProductsTab() {
  const [includeArchived, setIncludeArchived] = useState(false);
  const [search, setSearch] = useState('');
  const [category, setCategory] = useState('all');
  const [editing, setEditing] = useState<AdminProduct | null>(null);
  const [creating, setCreating] = useState(false);
  const [archiving, setArchiving] = useState<AdminProduct | null>(null);
  const products = useAdminProducts(includeArchived);
  const archive = useArchiveProduct();
  const restore = useRestoreProduct();
  const toast = useToast();
  const pricingRules = usePricingRules();

  const list = products.data ?? [];
  const categories = useMemo(() => uniqueSorted(list.map((p) => p.category)), [list]);
  const brands = useMemo(() => uniqueSorted(list.map((p) => p.brand)), [list]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return list.filter((p) => {
      if (category !== 'all' && p.category !== category) return false;
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        (p.tamilName ?? '').toLowerCase().includes(q) ||
        p.id.toLowerCase().includes(q) ||
        p.category.toLowerCase().includes(q) ||
        p.brand.toLowerCase().includes(q)
      );
    });
  }, [list, search, category]);

  const doRestore = (p: AdminProduct) =>
    restore.mutate(p.id, {
      onSuccess: () => toast.success(`Restored “${p.name}” — visible on the storefront again.`),
      onError: (err) => toast.error(`Could not restore “${p.name}”: ${errorMessage(err)}`),
    });

  const doArchive = () => {
    if (!archiving) return;
    const p = archiving;
    archive.mutate(p.id, {
      onSuccess: () => {
        toast.success(`Archived “${p.name}”. It is hidden from the storefront.`);
        setArchiving(null);
      },
    });
  };

  return (
    <div className="space-y-5">
      <div className="flex flex-col sm:flex-row sm:items-end justify-between gap-4">
        <div>
          <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">Products</h2>
          <p className="text-xs text-slate-500 font-semibold mt-0.5">
            The storefront catalogue. Archived products are hidden from customers but kept for order history.
          </p>
        </div>
        <Button variant="accent" icon={Plus} onClick={() => setCreating(true)} size="lg">
          Add product
        </Button>
      </div>

      <div className="flex flex-col md:flex-row gap-2 md:items-center">
        <div className="relative flex-1">
          <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" aria-hidden />
          <label htmlFor="products-search" className="sr-only">
            Search products
          </label>
          <input
            id="products-search"
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search name, Tamil name, ID, category or brand"
            className={`${inputClass} pl-10`}
          />
        </div>
        <label htmlFor="products-category" className="sr-only">
          Category
        </label>
        <select
          id="products-category"
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          className={`${inputClass} md:w-64`}
        >
          <option value="all">All categories</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
        <label className="inline-flex items-center gap-2 text-xs font-bold text-slate-700 bg-white border border-slate-200 rounded-xl px-3.5 py-2.5 cursor-pointer whitespace-nowrap">
          <input
            type="checkbox"
            checked={includeArchived}
            onChange={(e) => setIncludeArchived(e.target.checked)}
            className="accent-amber-500 w-4 h-4"
          />
          Show archived
        </label>
      </div>

      {products.isPending ? (
        <div className="space-y-2" aria-busy="true">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-16 rounded-2xl" />
          ))}
        </div>
      ) : products.isError && !products.data ? (
        <ErrorCard title="Could not load products" error={products.error} onRetry={() => void products.refetch()} retrying={products.isFetching} />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={ShoppingBag}
          title={list.length === 0 ? 'No products yet' : 'No products match'}
          description={list.length === 0 ? 'Add your first product to start selling.' : 'Try another search or category.'}
          action={
            list.length === 0 ? (
              <Button variant="accent" icon={Plus} onClick={() => setCreating(true)}>
                Add product
              </Button>
            ) : (
              <Button
                icon={X}
                onClick={() => {
                  setSearch('');
                  setCategory('all');
                }}
              >
                Clear filters
              </Button>
            )
          }
        />
      ) : (
        <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2.5 border-b border-slate-100 text-[11px] font-bold text-slate-500">
            <span>
              {filtered.length} of {list.length} product{list.length === 1 ? '' : 's'}
            </span>
            {products.isFetching && <Spinner className="w-3.5 h-3.5" label="Refreshing…" />}
          </div>
          <div className="overflow-x-auto">
            <table className={`w-full text-xs ${products.isPlaceholderData ? 'opacity-60' : ''}`}>
              <thead className="bg-slate-50 text-[10px] uppercase tracking-wider text-slate-500 text-left">
                <tr>
                  <th scope="col" className="px-4 py-3 font-extrabold">Product</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Category</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">
                    Price
                    <span className="block font-semibold normal-case tracking-normal text-slate-400">bulk / std</span>
                  </th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Stock</th>
                  <th scope="col" className="px-4 py-3 font-extrabold">Tags</th>
                  <th scope="col" className="px-4 py-3 font-extrabold text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {filtered.map((p) => (
                  <tr key={p.id} className={`align-top hover:bg-slate-50/60 ${!p.isActive ? 'bg-slate-50/80' : ''}`}>
                    <td className="px-4 py-3">
                      <div className="flex items-start gap-3 min-w-[220px]">
                        <ProductImage src={p.image} alt={p.name} className={`w-12 h-12 ${!p.isActive ? 'opacity-50 grayscale' : ''}`} />
                        <div className="min-w-0">
                          <p className="font-extrabold text-slate-900">{p.name}</p>
                          {p.tamilName && <p className="text-[11px] text-slate-500">{p.tamilName}</p>}
                          <p className="text-[10px] text-slate-400 mt-0.5">
                            <span className="font-mono">{p.id}</span> · {p.packInfo}
                            {p.bayLocation ? ` · ${p.bayLocation}` : ''}
                          </p>
                          {!p.isActive && (
                            <div className="mt-1">
                              <ArchivedBadge />
                            </div>
                          )}
                        </div>
                      </div>
                    </td>
                    <td className="px-4 py-3 text-slate-600">
                      <p className="font-bold">{p.category}</p>
                      <p className="text-[11px] text-slate-400">{p.brand}</p>
                    </td>
                    <td className="px-4 py-3 text-right whitespace-nowrap">
                      <p className="font-black text-slate-950 tabular-nums">{formatINR(p.price)}</p>
                      {p.originalPrice > p.price && (
                        <p className="text-[11px] text-slate-400 tabular-nums">
                          <span className="line-through">{formatINR(p.originalPrice)}</span>{' '}
                          <span className="text-emerald-700 font-bold">{p.discountPercent}% off</span>
                        </p>
                      )}
                      <StandardPriceLine p={p} standardDiscountPercent={pricingRules.standardDiscountPercent} />
                    </td>
                    <td className="px-4 py-3 text-right">
                      <StockCell p={p} />
                    </td>
                    <td className="px-4 py-3">
                      <Flags p={p} />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1.5">
                        <Button size="sm" icon={Pencil} onClick={() => setEditing(p)} aria-label={`Edit ${p.name}`}>
                          Edit
                        </Button>
                        {p.isActive ? (
                          <Button size="sm" variant="dangerOutline" icon={Archive} onClick={() => setArchiving(p)} aria-label={`Archive ${p.name}`}>
                            Archive
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="secondary"
                            icon={ArchiveRestore}
                            loading={restore.isPending && restore.variables === p.id}
                            disabled={restore.isPending}
                            onClick={() => doRestore(p)}
                            aria-label={`Restore ${p.name}`}
                          >
                            Restore
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <ProductFormModal
        open={creating || editing !== null}
        product={editing}
        categories={categories}
        brands={brands}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />

      <ConfirmDialog
        open={archiving !== null}
        title={`Archive “${archiving?.name ?? ''}”?`}
        confirmLabel="Archive product"
        busy={archive.isPending}
        error={archive.isError ? errorMessage(archive.error) : null}
        onCancel={() => {
          setArchiving(null);
          archive.reset();
        }}
        onConfirm={doArchive}
        description={
          <>
            It will be hidden from the storefront immediately and can no longer be ordered. Existing orders, stock
            ({formatCount(archiving?.stock ?? 0)} units) and history are kept. You can restore it at any time from{' '}
            <strong>Show archived</strong>.
          </>
        }
      />
    </div>
  );
}
