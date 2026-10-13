import React, { useEffect, useId, useState } from 'react';
import { AlertTriangle, ArrowRight, Equal, Minus, PackageCheck, Plus } from 'lucide-react';
import { isApiError } from '../../lib/api';
import { useInventory } from '../../hooks/admin/adminQueries';
import { useAdjustStock } from '../../hooks/admin/adminMutations';
import type { InventoryItem, StockAdjustMode } from '../../types/admin';
import { formatCount } from './format';
import { useToast } from './Toast';
import { Button, FieldError, FormError, inputClass, inputErrorClass, labelClass, Modal, ProductImage } from './ui';

const MAX_STOCK = 10_000_000;
const MAX_ADJUST = 1_000_000;

const MODES: { mode: StockAdjustMode; label: string; icon: React.ComponentType<{ className?: string }>; help: string }[] = [
  { mode: 'add', label: 'Restock (+)', icon: Plus, help: 'Add units received from the factory or returned.' },
  { mode: 'remove', label: 'Remove / write-off (−)', icon: Minus, help: 'Remove damaged, lost or sampled units.' },
  { mode: 'set', label: 'Set exact count', icon: Equal, help: 'Set stock to a physical count. Fails safely if an order changes stock meanwhile.' },
];

export function StockAdjustModal({ productId, onClose }: { productId: string | null; onClose: () => void }) {
  const inventory = useInventory();
  const item: InventoryItem | undefined = inventory.data?.items.find((i) => i.productId === productId);
  const open = productId !== null;

  const [mode, setMode] = useState<StockAdjustMode>('add');
  const [quantity, setQuantity] = useState('');
  const [note, setNote] = useState('');
  const [seenStock, setSeenStock] = useState<number>(0);
  const [qtyError, setQtyError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const mutation = useAdjustStock();
  const toast = useToast();
  const qtyId = useId();
  const noteId = useId();

  useEffect(() => {
    if (!open) return;
    setMode('add');
    setQuantity('');
    setNote('');
    setSeenStock(item?.stock ?? 0);
    setQtyError(null);
    setFormError(null);
    setConflict(null);
    mutation.reset();
    // Snapshot only when (re)opened for a product.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, productId]);

  if (!open) return null;

  const liveStock = item?.stock ?? seenStock;
  const baseStock = mode === 'set' ? seenStock : liveStock;
  const qtyNum = /^\d+$/.test(quantity.trim()) ? Number(quantity.trim()) : NaN;
  const preview = Number.isNaN(qtyNum)
    ? null
    : mode === 'add'
      ? baseStock + qtyNum
      : mode === 'remove'
        ? baseStock - qtyNum
        : qtyNum;
  const stockMovedSinceOpen = mode === 'set' && item && item.stock !== seenStock;

  const validateQty = (): string | null => {
    if (!quantity.trim()) return 'Enter a quantity.';
    if (Number.isNaN(qtyNum)) return 'Whole numbers only.';
    if (mode === 'set') {
      if (qtyNum < 0 || qtyNum > MAX_STOCK) return `Between 0 and ${formatCount(MAX_STOCK)}.`;
      if (qtyNum === seenStock) return 'That is already the current stock.';
    } else {
      if (qtyNum < 1) return 'At least 1.';
      if (qtyNum > MAX_ADJUST) return `At most ${formatCount(MAX_ADJUST)} per adjustment.`;
      if (mode === 'add' && liveStock + qtyNum > MAX_STOCK) return `Stock cannot exceed ${formatCount(MAX_STOCK)}.`;
      if (mode === 'remove' && qtyNum > liveStock) return `Only ${formatCount(liveStock)} in stock.`;
    }
    return null;
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!productId) return;
    const qErr = validateQty();
    setQtyError(qErr);
    if (note.trim().length > 300) return setFormError('Note must be at most 300 characters.');
    if (qErr) return;
    setFormError(null);
    setConflict(null);
    mutation.mutate(
      {
        productId,
        input: {
          mode,
          quantity: qtyNum,
          note: note.trim() || null,
          ...(mode === 'set' ? { expectedStock: seenStock } : {}),
        },
      },
      {
        onSuccess: (p) => {
          toast.success(`${p.name}: stock is now ${formatCount(p.stock)}.`);
          onClose();
        },
        onError: async (err) => {
          if (isApiError(err) && err.code === 'version_conflict') {
            const res = await inventory.refetch();
            const latest = res.data?.items.find((i) => i.productId === productId)?.stock;
            if (latest !== undefined) setSeenStock(latest);
            setConflict(
              `Stock changed while you were editing (probably an order or cancellation) — it is now ${
                latest !== undefined ? formatCount(latest) : 'different'
              } (you saw ${formatCount(seenStock)}). Check the count and submit again.`
            );
            return;
          }
          if (isApiError(err) && err.code === 'out_of_stock') {
            const available = err.details?.[0]?.available;
            void inventory.refetch();
            setQtyError(
              available !== undefined
                ? `Only ${formatCount(available)} in stock — cannot remove ${formatCount(qtyNum)}.`
                : err.message
            );
            return;
          }
          setFormError(isApiError(err) ? err.message : 'Something went wrong. Please try again.');
        },
      }
    );
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      icon={PackageCheck}
      title="Adjust stock"
      description={item ? `${item.name} · ${item.productId}${item.bayLocation ? ` · ${item.bayLocation}` : ''}` : productId}
      dismissable={!mutation.isPending}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" form="stock-adjust-form" loading={mutation.isPending}>
            {mode === 'add' ? 'Add stock' : mode === 'remove' ? 'Remove stock' : 'Set stock'}
          </Button>
        </>
      }
    >
      <form id="stock-adjust-form" onSubmit={submit} noValidate className="space-y-5">
        {item && (
          <div className="flex items-center gap-3 p-3 rounded-2xl bg-slate-50 border border-slate-200">
            <ProductImage src={item.image} alt={item.name} className="w-12 h-12" />
            <div className="flex-1">
              <p className="text-[11px] font-bold text-slate-500 uppercase tracking-wider">Current stock</p>
              <p className="text-xl font-black text-slate-950 tabular-nums">{formatCount(liveStock)}</p>
            </div>
            <p className="text-[11px] text-slate-500 text-right">
              Low-stock level
              <br />
              <span className="font-black text-slate-800 tabular-nums">{formatCount(item.lowStockThreshold)}</span>
            </p>
          </div>
        )}

        <div role="radiogroup" aria-label="Adjustment type" className="grid grid-cols-1 sm:grid-cols-3 gap-2">
          {MODES.map(({ mode: m, label, icon: Icon }) => (
            <button
              key={m}
              type="button"
              role="radio"
              aria-checked={mode === m}
              onClick={() => {
                setMode(m);
                setQtyError(null);
                setConflict(null);
                if (m === 'set') setSeenStock(liveStock);
              }}
              className={`flex items-center justify-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-bold border transition-all ${
                mode === m
                  ? 'bg-slate-950 text-white border-slate-950'
                  : 'bg-white text-slate-700 border-slate-200 hover:bg-slate-50'
              }`}
            >
              <Icon className="w-3.5 h-3.5" />
              {label}
            </button>
          ))}
        </div>
        <p className="text-[11px] text-slate-500 -mt-2">{MODES.find((m) => m.mode === mode)?.help}</p>

        {conflict && (
          <div role="alert" className="p-3 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs font-bold flex gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0 mt-px" />
            <span>{conflict}</span>
          </div>
        )}
        {stockMovedSinceOpen && !conflict && (
          <div className="p-3 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs font-bold flex flex-wrap items-center gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            <span className="flex-1">
              Stock just changed from {formatCount(seenStock)} to {formatCount(item.stock)}.
            </span>
            <Button size="sm" onClick={() => setSeenStock(item.stock)}>
              Use latest
            </Button>
          </div>
        )}

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 items-start">
          <div>
            <label htmlFor={qtyId} className={labelClass}>
              {mode === 'set' ? 'New stock count' : 'Quantity'} <span className="text-rose-500">*</span>
            </label>
            <input
              id={qtyId}
              inputMode="numeric"
              value={quantity}
              onChange={(e) => {
                setQuantity(e.target.value.replace(/[^\d]/g, ''));
                setQtyError(null);
              }}
              placeholder={mode === 'set' ? String(seenStock) : '0'}
              aria-invalid={!!qtyError}
              aria-describedby={qtyError ? `${qtyId}-err` : undefined}
              className={`${inputClass} text-base font-black tabular-nums ${qtyError ? inputErrorClass : ''}`}
            />
            <FieldError id={`${qtyId}-err`} message={qtyError} />
          </div>
          <div>
            <span className={labelClass}>Result</span>
            <div
              aria-live="polite"
              className="flex items-center gap-2 px-3.5 py-2.5 rounded-xl bg-slate-50 border border-slate-200 text-base font-black tabular-nums"
            >
              <span className="text-slate-500">{formatCount(baseStock)}</span>
              <ArrowRight className="w-4 h-4 text-slate-400" aria-hidden />
              <span
                className={
                  preview === null
                    ? 'text-slate-300'
                    : preview < 0
                      ? 'text-rose-600'
                      : preview === 0
                        ? 'text-rose-600'
                        : item && preview <= item.lowStockThreshold
                          ? 'text-amber-700'
                          : 'text-emerald-700'
                }
              >
                {preview === null ? '—' : formatCount(preview)}
              </span>
              {preview !== null && preview !== baseStock && (
                <span className="text-[11px] font-bold text-slate-500 ml-auto">
                  {preview - baseStock > 0 ? '+' : ''}
                  {formatCount(preview - baseStock)}
                </span>
              )}
            </div>
          </div>
        </div>

        <div>
          <label htmlFor={noteId} className={labelClass}>
            Note
          </label>
          <textarea
            id={noteId}
            value={note}
            onChange={(e) => setNote(e.target.value)}
            rows={2}
            maxLength={300}
            placeholder={mode === 'add' ? 'e.g. Batch #12 from factory' : mode === 'remove' ? 'e.g. Damaged in rain' : 'e.g. Physical count 13 Oct'}
            className={`${inputClass} resize-none`}
          />
          <p className="text-[10px] text-slate-400 text-right tabular-nums">{note.length}/300</p>
        </div>
        <FormError message={formError} />
      </form>
    </Modal>
  );
}
