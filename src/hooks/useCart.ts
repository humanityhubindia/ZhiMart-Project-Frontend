import { useCallback, useEffect, useMemo, useState } from 'react';
import { CartItem, CrackerProduct } from '../types';
import { computeTotals } from '../lib/money';
import { usePricingRules } from './useProducts';

/**
 * In-memory shopping cart (React state only — never persisted to storage).
 *
 * Every time the live catalog changes, each line is reconciled against it:
 * - the product snapshot is refreshed (price, stock flags);
 * - quantity above `availableQuantity` is lowered and `adjustedFrom` records the old value;
 * - products that are archived/unknown or out of stock are flagged `unavailable`
 *   (checkout is blocked until they are removed).
 */
export function useCart(products: CrackerProduct[] | undefined) {
  const [items, setItems] = useState<CartItem[]>([]);
  const pricing = usePricingRules();

  const productsById = useMemo(
    () => (products ? new Map(products.map((p) => [p.id, p] as const)) : null),
    [products]
  );

  // Reconcile whenever a fresh catalog arrives.
  useEffect(() => {
    if (!productsById) return;
    setItems((prev) => {
      let changed = false;
      const next = prev.map((line) => {
        const live = productsById.get(line.product.id);
        if (!live || !live.inStock || live.availableQuantity <= 0) {
          const product = live ?? line.product;
          if (line.unavailable && line.product === product) return line;
          changed = true;
          return { ...line, product, unavailable: true };
        }
        let updated: CartItem = line;
        if (line.product !== live || line.unavailable) {
          updated = { ...updated, product: live, unavailable: false };
        }
        if (line.quantity > live.availableQuantity) {
          updated = {
            ...updated,
            quantity: live.availableQuantity,
            adjustedFrom: line.adjustedFrom ?? line.quantity,
          };
        }
        if (updated !== line) changed = true;
        return updated;
      });
      return changed ? next : prev;
    });
  }, [productsById]);

  /** Max orderable quantity for a product, using the freshest data we have. */
  const maxFor = useCallback(
    (product: CrackerProduct) => {
      const live = productsById?.get(product.id) ?? product;
      return live.inStock ? Math.max(0, live.availableQuantity) : 0;
    },
    [productsById]
  );

  const addToCart = useCallback(
    (product: CrackerProduct, quantity = 1) => {
      const max = maxFor(product);
      if (max <= 0 || quantity <= 0) return;
      const live = productsById?.get(product.id) ?? product;
      setItems((prev) => {
        const existing = prev.find((i) => i.product.id === product.id);
        if (existing) {
          const nextQty = Math.min(max, existing.quantity + quantity);
          if (nextQty === existing.quantity) return prev;
          return prev.map((i) =>
            i.product.id === product.id
              ? { product: live, quantity: nextQty }
              : i
          );
        }
        return [...prev, { product: live, quantity: Math.min(max, quantity) }];
      });
    },
    [maxFor, productsById]
  );

  const removeItem = useCallback((productId: string) => {
    setItems((prev) => prev.filter((i) => i.product.id !== productId));
  }, []);

  const updateQuantity = useCallback(
    (productId: string, quantity: number) => {
      if (quantity <= 0) {
        removeItem(productId);
        return;
      }
      setItems((prev) =>
        prev.map((i) => {
          if (i.product.id !== productId) return i;
          const max = maxFor(i.product);
          const nextQty = Math.min(Math.floor(quantity), max);
          if (nextQty <= 0) return { ...i, unavailable: true };
          // Any explicit change acknowledges a previous automatic adjustment.
          return { product: i.product, quantity: nextQty, unavailable: i.unavailable };
        })
      );
    },
    [maxFor, removeItem]
  );

  const addMany = useCallback(
    (lines: { product: CrackerProduct; quantity: number }[]) => {
      lines.forEach(({ product, quantity }) => addToCart(product, quantity));
    },
    [addToCart]
  );

  const clearCart = useCallback(() => setItems([]), []);

  const quantityOf = useCallback(
    (productId: string) => items.find((i) => i.product.id === productId)?.quantity ?? 0,
    [items]
  );

  const totals = useMemo(() => computeTotals(items, pricing), [items, pricing]);
  const hasUnavailable = items.some((i) => i.unavailable);

  return {
    items,
    totals,
    hasUnavailable,
    addToCart,
    addMany,
    updateQuantity,
    removeItem,
    clearCart,
    quantityOf,
    maxFor,
  };
}
