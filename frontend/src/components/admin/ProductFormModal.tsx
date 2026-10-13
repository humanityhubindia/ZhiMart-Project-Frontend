import React, { useEffect, useId, useMemo, useState } from 'react';
import { AlertTriangle, ImagePlus, PackagePlus, Pencil, Plus, Trash2 } from 'lucide-react';
import { isApiError } from '../../lib/api';
import { useAdminApi } from '../../hooks/admin/adminQueries';
import { useCreateProduct, useUpdateProduct } from '../../hooks/admin/adminMutations';
import type { AdminProduct, ProductInput, SoundLevel } from '../../types/admin';
import { SOUND_LEVELS } from '../../types/admin';
import { usePricingRules } from '../../hooks/useProducts';
import { formatCount, formatDateTime, formatINR, PRODUCT_ASSET_IMAGES, standardPriceFor, validateImageRef } from './format';
import { useToast } from './Toast';
import {
  Button,
  FieldError,
  FormError,
  inputClass,
  inputErrorClass,
  labelClass,
  Modal,
  ProductImage,
} from './ui';

// ---------------------------------------------------------------------------
// Form model
// ---------------------------------------------------------------------------

interface FormState {
  id: string;
  name: string;
  tamilName: string;
  category: string;
  brand: string;
  packInfo: string;
  piecesPerPack: string;
  price: string;
  originalPrice: string;
  image: string;
  gallery: string[];
  description: string;
  soundLevel: SoundLevel | '';
  isPopular: boolean;
  isBestSelling: boolean;
  isNewArrival: boolean;
  isParty: boolean;
  lowStockThreshold: string;
  bayLocation: string;
  sortOrder: string;
  initialStock: string;
}

type FieldKey = keyof FormState;
type Errors = Partial<Record<FieldKey, string>>;

const EMPTY: FormState = {
  id: '',
  name: '',
  tamilName: '',
  category: '',
  brand: 'Zhi-mart Fireworks',
  packInfo: '',
  piecesPerPack: '',
  price: '',
  originalPrice: '',
  image: PRODUCT_ASSET_IMAGES[0].url,
  gallery: [],
  description: '',
  soundLevel: '',
  isPopular: false,
  isBestSelling: false,
  isNewArrival: false,
  isParty: false,
  lowStockThreshold: '20',
  bayLocation: '',
  sortOrder: '0',
  initialStock: '0',
};

function fromProduct(p: AdminProduct): FormState {
  return {
    id: p.id,
    name: p.name,
    tamilName: p.tamilName ?? '',
    category: p.category,
    brand: p.brand,
    packInfo: p.packInfo,
    piecesPerPack: p.piecesPerPack != null ? String(p.piecesPerPack) : '',
    price: String(p.price),
    originalPrice: String(p.originalPrice),
    image: p.image,
    gallery: [...(p.gallery ?? [])],
    description: p.description ?? '',
    soundLevel: p.soundLevel ?? '',
    isPopular: p.isPopular,
    isBestSelling: p.isBestSelling,
    isNewArrival: p.isNewArrival,
    isParty: p.isParty,
    lowStockThreshold: String(p.lowStockThreshold),
    bayLocation: p.bayLocation ?? '',
    sortOrder: String(p.sortOrder),
    initialStock: '0',
  };
}

const MONEY_RE = /^\d+(\.\d{1,2})?$/;
const INT_RE = /^-?\d+$/;
const ID_RE = /^[a-z0-9][a-z0-9-]{1,39}$/;
const MAX_PRICE = 1_000_000;
const MAX_STOCK = 10_000_000;
const MAX_LOW_STOCK_THRESHOLD = 100_000;
const SORT_ORDER_LIMIT = 1_000_000;

function validate(f: FormState, isCreate: boolean): Errors {
  const e: Errors = {};
  const len = (v: string) => v.trim().length;

  if (isCreate && f.id.trim() && !ID_RE.test(f.id.trim()))
    e.id = '2–40 chars: lowercase letters, digits and hyphens; must start with a letter or digit.';
  if (!len(f.name)) e.name = 'Name is required.';
  else if (len(f.name) > 120) e.name = 'At most 120 characters.';
  if (len(f.tamilName) > 120) e.tamilName = 'At most 120 characters.';
  if (!len(f.category)) e.category = 'Category is required.';
  else if (len(f.category) > 80) e.category = 'At most 80 characters.';
  if (!len(f.brand)) e.brand = 'Brand is required.';
  else if (len(f.brand) > 80) e.brand = 'At most 80 characters.';
  if (!len(f.packInfo)) e.packInfo = 'Pack info is required (e.g. "10 Packet(s)").';
  else if (len(f.packInfo) > 60) e.packInfo = 'At most 60 characters.';

  if (f.piecesPerPack.trim()) {
    const n = Number(f.piecesPerPack);
    if (!INT_RE.test(f.piecesPerPack.trim()) || n <= 0 || n > 100_000) e.piecesPerPack = 'Whole number above 0.';
  }

  const price = Number(f.price);
  const mrp = Number(f.originalPrice);
  if (!f.price.trim()) e.price = 'Selling price is required.';
  else if (!MONEY_RE.test(f.price.trim()) || price <= 0) e.price = 'Enter an amount above 0 with at most 2 decimals.';
  else if (price > MAX_PRICE) e.price = 'At most ₹10,00,000.';
  if (!f.originalPrice.trim()) e.originalPrice = 'MRP is required.';
  else if (!MONEY_RE.test(f.originalPrice.trim()) || mrp <= 0) e.originalPrice = 'Enter an amount above 0 with at most 2 decimals.';
  else if (mrp > MAX_PRICE) e.originalPrice = 'At most ₹10,00,000.';
  if (!e.price && !e.originalPrice && price > mrp) e.price = 'Selling price cannot exceed the MRP.';

  const imgErr = validateImageRef(f.image);
  if (imgErr) e.image = imgErr;
  if (f.gallery.length > 10) e.gallery = 'At most 10 gallery images.';
  else {
    const bad = f.gallery.findIndex((g) => validateImageRef(g) !== null);
    if (bad >= 0) e.gallery = `Gallery image ${bad + 1}: ${validateImageRef(f.gallery[bad])}`;
  }
  if (len(f.description) > 4000) e.description = 'At most 4000 characters.';

  const threshold = Number(f.lowStockThreshold);
  if (!INT_RE.test(f.lowStockThreshold.trim()) || threshold < 0 || threshold > MAX_LOW_STOCK_THRESHOLD)
    e.lowStockThreshold = `Whole number between 0 and ${formatCount(MAX_LOW_STOCK_THRESHOLD)}.`;
  if (len(f.bayLocation) > 80) e.bayLocation = 'At most 80 characters.';
  const sort = Number(f.sortOrder);
  if (!INT_RE.test(f.sortOrder.trim()) || Math.abs(sort) > SORT_ORDER_LIMIT)
    e.sortOrder = `Whole number between −${formatCount(SORT_ORDER_LIMIT)} and ${formatCount(SORT_ORDER_LIMIT)}.`;
  if (isCreate) {
    const stock = Number(f.initialStock);
    if (!INT_RE.test(f.initialStock.trim()) || stock < 0 || stock > MAX_STOCK)
      e.initialStock = `Whole number between 0 and ${formatCount(MAX_STOCK)}.`;
  }
  return e;
}

function toInput(f: FormState, isCreate: boolean, version?: number): ProductInput {
  const opt = (v: string) => (v.trim() ? v.trim() : null);
  const input: ProductInput = {
    name: f.name.trim(),
    tamilName: opt(f.tamilName),
    category: f.category.trim(),
    brand: f.brand.trim(),
    packInfo: f.packInfo.trim(),
    piecesPerPack: f.piecesPerPack.trim() ? Number(f.piecesPerPack) : null,
    price: Number(f.price),
    originalPrice: Number(f.originalPrice),
    image: f.image.trim(),
    gallery: f.gallery.map((g) => g.trim()).filter(Boolean),
    description: f.description.trim(),
    soundLevel: f.soundLevel || null,
    isPopular: f.isPopular,
    isBestSelling: f.isBestSelling,
    isNewArrival: f.isNewArrival,
    isParty: f.isParty,
    lowStockThreshold: Number(f.lowStockThreshold),
    bayLocation: opt(f.bayLocation),
    sortOrder: Number(f.sortOrder),
  };
  if (isCreate) {
    if (f.id.trim()) input.id = f.id.trim();
    input.initialStock = Number(f.initialStock);
  } else {
    input.expectedVersion = version;
  }
  return input;
}

/** Field names the server may mention in `validation_error` messages (longest first). */
const SERVER_FIELD_ALIASES: [FieldKey, string[]][] = [
  ['lowStockThreshold', ['lowStockThreshold', 'low_stock_threshold', 'low stock threshold']],
  ['originalPrice', ['originalPrice', 'original_price', 'mrp']],
  ['piecesPerPack', ['piecesPerPack', 'pieces_per_pack', 'pieces per pack']],
  ['initialStock', ['initialStock', 'initial_stock', 'initial stock']],
  ['bayLocation', ['bayLocation', 'bay_location', 'bay location']],
  ['description', ['description']],
  ['soundLevel', ['soundLevel', 'sound_level', 'sound level']],
  ['tamilName', ['tamilName', 'tamil_name', 'tamil name']],
  ['sortOrder', ['sortOrder', 'sort_order', 'sort order']],
  ['packInfo', ['packInfo', 'pack_info', 'pack info']],
  ['category', ['category']],
  ['gallery', ['gallery']],
  ['brand', ['brand']],
  ['image', ['image', 'image_url']],
  ['price', ['price']],
  ['name', ['name']],
  ['id', ['id']],
];

function fieldFromServerMessage(message: string): FieldKey | null {
  for (const [key, aliases] of SERVER_FIELD_ALIASES) {
    for (const alias of aliases) {
      const re = new RegExp(`(^|[^A-Za-z0-9_])${alias.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}($|[^A-Za-z0-9_])`, 'i');
      if (re.test(message)) return key;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Small field components
// ---------------------------------------------------------------------------

function TextField({
  label,
  field,
  value,
  onChange,
  error,
  required,
  hint,
  maxLength,
  inputMode,
  placeholder,
  prefix,
  className = '',
  mono,
  disabled,
  list,
}: {
  label: string;
  field: FieldKey;
  value: string;
  onChange: (v: string) => void;
  error?: string;
  required?: boolean;
  hint?: React.ReactNode;
  maxLength?: number;
  inputMode?: React.HTMLAttributes<HTMLInputElement>['inputMode'];
  placeholder?: string;
  prefix?: string;
  className?: string;
  mono?: boolean;
  disabled?: boolean;
  list?: string;
}) {
  const id = `pf-${field}`;
  return (
    <div className={className}>
      <label htmlFor={id} className={labelClass}>
        {label}
        {required && <span className="text-rose-500"> *</span>}
      </label>
      <div className="relative">
        {prefix && (
          <span className="absolute left-3.5 top-1/2 -translate-y-1/2 text-xs font-bold text-slate-400" aria-hidden>
            {prefix}
          </span>
        )}
        <input
          id={id}
          name={field}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          maxLength={maxLength}
          inputMode={inputMode}
          placeholder={placeholder}
          disabled={disabled}
          list={list}
          aria-invalid={!!error}
          aria-required={required}
          aria-describedby={[error ? `${id}-err` : '', hint ? `${id}-hint` : ''].filter(Boolean).join(' ') || undefined}
          className={`${inputClass} ${prefix ? 'pl-7' : ''} ${mono ? 'font-mono' : ''} ${error ? inputErrorClass : ''}`}
        />
      </div>
      {hint && !error && (
        <p id={`${id}-hint`} className="mt-1 text-[11px] text-slate-400">
          {hint}
        </p>
      )}
      <FieldError id={`${id}-err`} message={error} />
    </div>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between gap-3 px-3 py-2.5 rounded-xl border border-slate-200 bg-white cursor-pointer hover:bg-slate-50">
      <span className="text-xs font-bold text-slate-800">{label}</span>
      <span className="relative inline-flex items-center">
        <input type="checkbox" className="peer sr-only" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <span className="w-9 h-5 rounded-full bg-slate-200 peer-checked:bg-amber-500 peer-focus-visible:ring-2 peer-focus-visible:ring-amber-500 peer-focus-visible:ring-offset-1 transition-colors" />
        <span className="absolute left-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform peer-checked:translate-x-4" />
      </span>
    </label>
  );
}

// ---------------------------------------------------------------------------
// Modal
// ---------------------------------------------------------------------------

export function ProductFormModal({
  open,
  product,
  categories,
  brands,
  onClose,
}: {
  open: boolean;
  /** `null` = create a new product. */
  product: AdminProduct | null;
  categories: string[];
  brands: string[];
  onClose: () => void;
}) {
  const isCreate = product === null;
  const [form, setForm] = useState<FormState>(EMPTY);
  const [version, setVersion] = useState<number | undefined>(undefined);
  const [base, setBase] = useState<AdminProduct | null>(null);
  const [errors, setErrors] = useState<Errors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [conflictNotice, setConflictNotice] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const create = useCreateProduct();
  const update = useUpdateProduct();
  const api = useAdminApi();
  const toast = useToast();
  const catListId = useId();
  const brandListId = useId();
  const busy = create.isPending || update.isPending;

  useEffect(() => {
    if (!open) return;
    setForm(product ? fromProduct(product) : EMPTY);
    setVersion(product?.version);
    setBase(product);
    setErrors({});
    setFormError(null);
    setConflictNotice(null);
    setSubmitted(false);
    // Reset when opened or a different product is chosen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, product?.id]);

  const set = <K extends FieldKey>(key: K, value: FormState[K]) => {
    setForm((f) => ({ ...f, [key]: value }));
  };

  // After the first submit attempt, re-validate live as the admin fixes fields.
  useEffect(() => {
    if (submitted) setErrors(validate(form, isCreate));
  }, [form, submitted, isCreate]);

  const discount = useMemo(() => {
    const p = Number(form.price);
    const m = Number(form.originalPrice);
    if (!(p > 0) || !(m > 0) || p > m) return null;
    return Math.round(((m - p) / m) * 100);
  }, [form.price, form.originalPrice]);

  const pricingRules = usePricingRules();
  const standardPreview = useMemo(() => {
    const p = Number(form.price);
    const m = Number(form.originalPrice);
    if (!(p > 0) || !(m > 0) || p > m) return null;
    const value = standardPriceFor(p, m, pricingRules.standardDiscountPercent);
    if (value === null) return null;
    return { value, percent: Math.round(((m - value) / m) * 100) };
  }, [form.price, form.originalPrice, pricingRules.standardDiscountPercent]);
  const bulkThreshold = formatINR(pricingRules.bulkMinOrder);

  const imageError = validateImageRef(form.image);

  const reloadLatest = async () => {
    if (!product) return;
    try {
      const all = await api.listProducts(true);
      const latest = all.find((p) => p.id === product.id);
      if (!latest) {
        setFormError('This product no longer exists.');
        return;
      }
      setForm(fromProduct(latest));
      setVersion(latest.version);
      setBase(latest);
      setErrors({});
      setConflictNotice(
        `Someone else saved this product at ${formatDateTime(latest.updatedAt)}. Your changes were not saved — the form now shows the latest version. Re-apply your edits and save again.`
      );
    } catch {
      setFormError('This product was changed elsewhere and the latest copy could not be loaded. Close and reopen it.');
    }
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    const errs = validate(form, isCreate);
    setErrors(errs);
    setFormError(null);
    if (Object.keys(errs).length) {
      const first = Object.keys(errs)[0];
      document.getElementById(`pf-${first}`)?.focus();
      return;
    }
    const input = toInput(form, isCreate, version);
    const onError = (err: unknown) => {
      if (!isApiError(err)) {
        setFormError('Something went wrong. Please try again.');
        return;
      }
      if (err.code === 'version_conflict') {
        void reloadLatest();
        return;
      }
      if (err.code === 'duplicate_id') {
        setErrors((x) => ({ ...x, id: 'A product with this ID already exists. Choose another ID.' }));
        if (!form.id.trim()) setFormError('The ID generated from this name already exists — enter a unique product ID.');
        return;
      }
      if (err.code === 'validation_error') {
        const field = fieldFromServerMessage(err.message);
        if (field) {
          setErrors((x) => ({ ...x, [field]: err.message }));
          document.getElementById(`pf-${field}`)?.focus();
          return;
        }
      }
      if (err.code === 'not_found') {
        setFormError('This product no longer exists.');
        return;
      }
      setFormError(err.message);
    };

    if (isCreate) {
      create.mutate(input, {
        onSuccess: (p) => {
          toast.success(`Added “${p.name}” (${p.id}) with ${formatCount(p.stock)} in stock.`);
          onClose();
        },
        onError,
      });
    } else if (product) {
      update.mutate(
        { id: product.id, input },
        {
          onSuccess: (p) => {
            toast.success(`Saved “${p.name}”.`);
            onClose();
          },
          onError,
        }
      );
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      dismissable={!busy}
      icon={isCreate ? PackagePlus : Pencil}
      title={isCreate ? 'Add product' : `Edit ${product?.name ?? 'product'}`}
      description={
        isCreate
          ? 'New products appear on the storefront as soon as they are saved.'
          : `${product?.id} · version ${version ?? '—'} · last saved ${formatDateTime(base?.updatedAt)}`
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="accent" type="submit" form="product-form" loading={busy}>
            {isCreate ? 'Create product' : 'Save changes'}
          </Button>
        </>
      }
    >
      <form id="product-form" onSubmit={submit} noValidate className="space-y-6">
        {conflictNotice && (
          <div role="alert" className="p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs font-bold flex gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0 mt-px" />
            <span>{conflictNotice}</span>
          </div>
        )}

        {/* Basics */}
        <fieldset className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <legend className="sr-only">Basics</legend>
          <TextField label="Name" field="name" value={form.name} onChange={(v) => set('name', v)} error={errors.name} required maxLength={120} placeholder={'e.g. 4" Lakshmi'} />
          <TextField label="Tamil name" field="tamilName" value={form.tamilName} onChange={(v) => set('tamilName', v)} error={errors.tamilName} maxLength={120} placeholder="Optional" />
          <TextField
            label="Category"
            field="category"
            value={form.category}
            onChange={(v) => set('category', v)}
            error={errors.category}
            required
            maxLength={80}
            list={catListId}
            placeholder="Pick or type a category"
          />
          <datalist id={catListId}>
            {categories.map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
          <TextField label="Brand" field="brand" value={form.brand} onChange={(v) => set('brand', v)} error={errors.brand} required maxLength={80} list={brandListId} />
          <datalist id={brandListId}>
            {brands.map((b) => (
              <option key={b} value={b} />
            ))}
          </datalist>
          <TextField label="Pack info" field="packInfo" value={form.packInfo} onChange={(v) => set('packInfo', v)} error={errors.packInfo} required maxLength={60} placeholder="e.g. 10 Packet(s)" />
          <TextField label="Pieces per pack" field="piecesPerPack" value={form.piecesPerPack} onChange={(v) => set('piecesPerPack', v)} error={errors.piecesPerPack} inputMode="numeric" placeholder="Optional" />
          {isCreate && (
            <TextField
              label="Product ID"
              field="id"
              value={form.id}
              onChange={(v) => set('id', v.toLowerCase())}
              error={errors.id}
              maxLength={40}
              mono
              placeholder="Auto-generated from name"
              hint="Optional. Lowercase letters, digits, hyphens (e.g. zm-21). Cannot be changed later."
              className="sm:col-span-2"
            />
          )}
        </fieldset>

        {/* Pricing */}
        <fieldset>
          <legend className="text-xs font-black text-slate-900 mb-3">Pricing</legend>
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
            <TextField label="MRP" field="originalPrice" value={form.originalPrice} onChange={(v) => set('originalPrice', v)} error={errors.originalPrice} required inputMode="decimal" prefix="₹" />
            <TextField label="Selling price" field="price" value={form.price} onChange={(v) => set('price', v)} error={errors.price} required inputMode="decimal" prefix="₹" hint={`Price-list (bulk) price, for orders of ${bulkThreshold}+. Must not exceed MRP.`} />
            <div>
              <span className={labelClass}>Discount</span>
              <div className="px-3.5 py-2.5 rounded-xl bg-slate-50 border border-slate-200 text-xs font-black text-emerald-700 tabular-nums" aria-live="polite">
                {discount === null ? <span className="text-slate-400 font-semibold">—</span> : `${discount}% off`}
              </div>
              <p className="mt-1 text-[11px] text-slate-400">Calculated automatically.</p>
            </div>
            <div className="sm:col-span-3 px-3.5 py-2.5 rounded-xl bg-sky-50/70 border border-sky-200 text-xs" aria-live="polite">
              <p className="font-bold text-slate-700">
                Standard price (below {bulkThreshold} orders):{' '}
                {standardPreview === null ? (
                  <span className="text-slate-400 font-semibold">—</span>
                ) : (
                  <span className="font-black text-slate-950 tabular-nums">
                    {formatINR(standardPreview.value)}{' '}
                    <span className="text-sky-800 font-bold">({standardPreview.percent}% off)</span>
                  </span>
                )}
              </p>
              <p className="mt-0.5 text-[11px] text-slate-500">
                Read-only — MRP less {pricingRules.standardDiscountPercent}%, rounded up to the rupee and never below the
                selling price. Customers pay it on every item while their order is under {bulkThreshold}.
              </p>
            </div>
          </div>
        </fieldset>

        {/* Image */}
        <fieldset>
          <legend className="text-xs font-black text-slate-900 mb-3">Images</legend>
          <div className="grid grid-cols-1 md:grid-cols-[1fr_160px] gap-4">
            <div className="space-y-3">
              <div role="radiogroup" aria-label="Choose a catalogue image" className="grid grid-cols-4 sm:grid-cols-7 gap-2">
                {PRODUCT_ASSET_IMAGES.map((img) => {
                  const selected = form.image.trim() === img.url;
                  return (
                    <button
                      key={img.url}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      title={img.label}
                      onClick={() => set('image', img.url)}
                      className={`rounded-xl p-0.5 border-2 transition-all ${selected ? 'border-amber-500' : 'border-transparent hover:border-slate-300'}`}
                    >
                      <ProductImage src={img.url} alt={img.label} className="w-full aspect-square" />
                    </button>
                  );
                })}
              </div>
              <TextField
                label="Main image"
                field="image"
                value={form.image}
                onChange={(v) => set('image', v)}
                error={errors.image}
                required
                maxLength={2048}
                mono
                placeholder="/assets/products/… or https://…"
                hint="Pick an image above, or enter a /assets/ path or an https:// URL."
              />
            </div>
            <div>
              <span className={labelClass}>Preview</span>
              {imageError ? (
                <div className="w-full aspect-square rounded-2xl border border-dashed border-slate-300 bg-slate-50 flex items-center justify-center text-[11px] text-slate-400 text-center p-3">
                  Enter a valid image path to preview
                </div>
              ) : (
                <ProductImage src={form.image.trim()} alt="Main image preview" className="w-full aspect-square !rounded-2xl" />
              )}
            </div>
          </div>

          <div className="mt-4">
            <div className="flex items-center justify-between">
              <span className={labelClass}>Gallery ({form.gallery.length}/10)</span>
              <Button
                size="sm"
                variant="ghost"
                icon={Plus}
                disabled={form.gallery.length >= 10}
                onClick={() => set('gallery', [...form.gallery, ''])}
              >
                Add image
              </Button>
            </div>
            {form.gallery.length === 0 ? (
              <p className="text-[11px] text-slate-400">No extra images. Optional.</p>
            ) : (
              <ul className="space-y-2">
                {form.gallery.map((g, i) => {
                  const err = g.trim() ? validateImageRef(g) : 'Enter a path or remove this row.';
                  return (
                    <li key={i} className="flex items-center gap-2">
                      {!err ? <ProductImage src={g.trim()} alt={`Gallery image ${i + 1}`} className="w-9 h-9" /> : (
                        <div className="w-9 h-9 rounded-xl bg-slate-50 border border-dashed border-slate-300 flex items-center justify-center shrink-0">
                          <ImagePlus className="w-3.5 h-3.5 text-slate-300" />
                        </div>
                      )}
                      <label className="sr-only" htmlFor={`pf-gallery-${i}`}>
                        Gallery image {i + 1}
                      </label>
                      <input
                        id={`pf-gallery-${i}`}
                        value={g}
                        maxLength={2048}
                        list="pf-asset-list"
                        onChange={(e) => {
                          const next = [...form.gallery];
                          next[i] = e.target.value;
                          set('gallery', next);
                        }}
                        placeholder="/assets/products/… or https://…"
                        aria-invalid={submitted && !!err}
                        className={`${inputClass} font-mono ${submitted && err ? inputErrorClass : ''}`}
                      />
                      <button
                        type="button"
                        onClick={() => set('gallery', form.gallery.filter((_, j) => j !== i))}
                        aria-label={`Remove gallery image ${i + 1}`}
                        className="p-2 rounded-lg text-slate-400 hover:text-rose-600 hover:bg-rose-50"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
            <datalist id="pf-asset-list">
              {PRODUCT_ASSET_IMAGES.map((img) => (
                <option key={img.url} value={img.url} />
              ))}
            </datalist>
            <span id="pf-gallery" tabIndex={-1} />
            <FieldError id="pf-gallery-err" message={errors.gallery} />
          </div>
        </fieldset>

        {/* Details */}
        <fieldset className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <legend className="text-xs font-black text-slate-900 mb-3 sm:col-span-2">Details</legend>
          <div className="sm:col-span-2">
            <label htmlFor="pf-description" className={labelClass}>
              Description
            </label>
            <textarea
              id="pf-description"
              value={form.description}
              onChange={(e) => set('description', e.target.value)}
              rows={3}
              maxLength={4000}
              aria-invalid={!!errors.description}
              aria-describedby={errors.description ? 'pf-description-err' : undefined}
              className={`${inputClass} resize-y ${errors.description ? inputErrorClass : ''}`}
            />
            <div className="flex">
              <FieldError id="pf-description-err" message={errors.description} />
              <span className="ml-auto text-[10px] text-slate-400 mt-1 tabular-nums">{form.description.length}/4000</span>
            </div>
          </div>
          <div>
            <label htmlFor="pf-soundLevel" className={labelClass}>
              Sound level
            </label>
            <select
              id="pf-soundLevel"
              value={form.soundLevel}
              onChange={(e) => set('soundLevel', e.target.value as SoundLevel | '')}
              aria-invalid={!!errors.soundLevel}
              className={`${inputClass} ${errors.soundLevel ? inputErrorClass : ''}`}
            >
              <option value="">Not specified</option>
              {SOUND_LEVELS.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
            <FieldError id="pf-soundLevel-err" message={errors.soundLevel} />
          </div>
          <TextField label="Sort order" field="sortOrder" value={form.sortOrder} onChange={(v) => set('sortOrder', v)} error={errors.sortOrder} inputMode="numeric" hint="Lower numbers show first on the storefront." />
          <div className="sm:col-span-2 grid grid-cols-2 lg:grid-cols-4 gap-2">
            <Toggle label="Popular" checked={form.isPopular} onChange={(v) => set('isPopular', v)} />
            <Toggle label="Best selling" checked={form.isBestSelling} onChange={(v) => set('isBestSelling', v)} />
            <Toggle label="New arrival" checked={form.isNewArrival} onChange={(v) => set('isNewArrival', v)} />
            <Toggle label="Party pick" checked={form.isParty} onChange={(v) => set('isParty', v)} />
          </div>
        </fieldset>

        {/* Stock */}
        <fieldset className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <legend className="text-xs font-black text-slate-900 mb-3 sm:col-span-3">Stock & warehouse</legend>
          {isCreate ? (
            <TextField label="Initial stock" field="initialStock" value={form.initialStock} onChange={(v) => set('initialStock', v)} error={errors.initialStock} required inputMode="numeric" hint="Recorded as the opening stock movement." />
          ) : (
            <div>
              <span className={labelClass}>Current stock</span>
              <div className="px-3.5 py-2.5 rounded-xl bg-slate-50 border border-slate-200 text-xs font-black text-slate-900 tabular-nums">
                {formatCount(base?.stock ?? 0)} units
              </div>
              <p className="mt-1 text-[11px] text-slate-400">Change stock from the Inventory tab.</p>
            </div>
          )}
          <TextField label="Low-stock threshold" field="lowStockThreshold" value={form.lowStockThreshold} onChange={(v) => set('lowStockThreshold', v)} error={errors.lowStockThreshold} required inputMode="numeric" hint="Flagged as low stock at or below this count." />
          <TextField label="Bay location" field="bayLocation" value={form.bayLocation} onChange={(v) => set('bayLocation', v)} error={errors.bayLocation} maxLength={80} placeholder="e.g. Bay B-1" />
        </fieldset>

        <FormError message={formError} />
      </form>
    </Modal>
  );
}
