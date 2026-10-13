/**
 * Public catalog endpoints (`GET /products`, `GET /products/{id}`) — see backend/API.md,
 * "Catalog, Inventory & Orders". The database is the only source of truth for products
 * and stock; nothing here is cached outside react-query.
 */
import { apiRequest } from './api';
import { CrackerProduct } from '../types';
import { DEFAULT_PRICING, PricingRules } from './money';

interface ProductsResponse {
  products: CrackerProduct[];
  /** Present (true) when the server runs in ₹1 payment test mode. */
  testMode?: boolean;
  /** Two-tier pricing rules (bulk vs standard price). */
  pricing?: Partial<PricingRules>;
}

export interface Catalog {
  products: CrackerProduct[];
  testMode: boolean;
  pricing: PricingRules;
}

/** A finite number ≥ 0 (0 is meaningful: `bulkMinOrder: 0` = every order gets the bulk price). */
const nonNegative = (v: unknown, fallback: number) => {
  if (v === null || v === undefined || v === '') return fallback;
  const n = Number(v);
  return Number.isFinite(n) && n >= 0 ? n : fallback;
};

export function normalizePricing(raw: Partial<PricingRules> | undefined): PricingRules {
  return {
    bulkMinOrder: nonNegative(raw?.bulkMinOrder, DEFAULT_PRICING.bulkMinOrder),
    bulkDiscountPercent: nonNegative(raw?.bulkDiscountPercent, DEFAULT_PRICING.bulkDiscountPercent),
    standardDiscountPercent: nonNegative(
      raw?.standardDiscountPercent,
      DEFAULT_PRICING.standardDiscountPercent
    ),
  };
}

/** `GET /storefront/config` — public storefront settings (the Razorpay key id is public by design). */
export interface StorefrontConfig {
  testMode: boolean;
  /** `RAZORPAY_ENABLED`: false ⇒ orders are "pay later" (team confirms payment by phone). */
  onlinePaymentsEnabled: boolean;
  razorpayKeyId: string | null;
  currency: string;
  paymentExpiryMinutes: number;
  storeName: string;
}

/** Defensive normalisation so a slightly-off payload can't crash the storefront. */
export function normalizeProduct(raw: CrackerProduct): CrackerProduct {
  const price = Number(raw.price) || 0;
  const originalPrice = Number(raw.originalPrice) || price;
  const availableQuantity = Math.max(0, Math.floor(Number(raw.availableQuantity) || 0));
  const inStock = Boolean(raw.inStock) && availableQuantity > 0;
  // Never below the bulk price; an old backend without the field falls back to the bulk price.
  const standardPrice = Math.max(price, Number(raw.standardPrice) || price);
  const standardDiscountPercent =
    originalPrice > 0 ? Math.round(((originalPrice - standardPrice) / originalPrice) * 100) : 0;
  const discountPercent =
    typeof raw.discountPercent === 'number'
      ? raw.discountPercent
      : originalPrice > 0
        ? Math.round(((originalPrice - price) / originalPrice) * 100)
        : 0;

  return {
    ...raw,
    price,
    originalPrice,
    discountPercent,
    standardPrice,
    standardDiscountPercent,
    gallery: Array.isArray(raw.gallery) ? raw.gallery.filter(Boolean) : [],
    description: raw.description ?? '',
    inStock,
    availableQuantity: inStock ? availableQuantity : 0,
  };
}

export async function fetchCatalog(signal?: AbortSignal): Promise<Catalog> {
  const res = await apiRequest<ProductsResponse>('/products', { signal });
  const pricing = normalizePricing(res?.pricing);
  let products = (res?.products ?? []).map(normalizeProduct);
  // No minimum (`BULK_MIN_ORDER_RUPEES=0`): every order is charged the bulk price, so the
  // standard price never applies — collapse it so cards/estimates don't advertise a second tier.
  if (pricing.bulkMinOrder <= 0) {
    products = products.map((p) => ({
      ...p,
      standardPrice: p.price,
      standardDiscountPercent: p.discountPercent,
    }));
  }
  return {
    products,
    testMode: res?.testMode === true,
    pricing,
  };
}

export async function fetchProducts(signal?: AbortSignal): Promise<CrackerProduct[]> {
  return (await fetchCatalog(signal)).products;
}

export async function fetchStorefrontConfig(signal?: AbortSignal): Promise<StorefrontConfig> {
  const res = await apiRequest<Partial<StorefrontConfig>>('/storefront/config', { signal });
  return {
    testMode: res?.testMode === true,
    onlinePaymentsEnabled: res?.onlinePaymentsEnabled === true,
    razorpayKeyId: res?.razorpayKeyId ?? null,
    currency: res?.currency ?? 'INR',
    paymentExpiryMinutes: Number(res?.paymentExpiryMinutes) || 30,
    storeName: res?.storeName || 'Zhi-Mart Fireworks',
  };
}

export async function fetchProduct(id: string, signal?: AbortSignal): Promise<CrackerProduct> {
  const res = await apiRequest<CrackerProduct>(`/products/${encodeURIComponent(id)}`, { signal });
  return normalizeProduct(res);
}
