import { useCallback, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ApiError, isApiError, StockShortage } from '../lib/api';
import {
  newIdempotencyKey,
  ORDER_LIMITS,
  placeOrder,
  PlaceOrderPayload,
  PlaceOrderResult,
  PlacedOrder,
  PricingTier,
} from '../lib/ordersApi';
import { computeTotals } from '../lib/money';
import { CheckoutField, CheckoutFormValues, mapServerValidation, toOrderCustomer } from '../lib/checkout';
import { CartItem } from '../types';
import { PRODUCTS_QUERY_KEY, usePricingRules } from './useProducts';

export type CheckoutFailure =
  | { kind: 'out_of_stock'; message: string; shortages: StockShortage[] }
  | { kind: 'product_unavailable'; message: string }
  | { kind: 'rate_limited'; message: string; retryAfter?: number }
  | { kind: 'validation'; message: string; field?: CheckoutField }
  | { kind: 'idempotency_conflict'; message: string }
  | { kind: 'network'; message: string }
  | { kind: 'unknown'; message: string };

export interface CheckoutSuccess {
  order: PlacedOrder;
  /** Guest access token — goes only into the order page URL fragment. */
  orderToken: string;
  customer: ReturnType<typeof toOrderCustomer>;
  notes: string | null;
  /** True when an earlier (ambiguous) attempt had already placed the order with other details. */
  recoveredPreviousAttempt?: boolean;
  /**
   * What the cart showed when the order was placed. Display only — the server re-prices every
   * order, and its `order.totalAmount` / `order.pricingTier` always win.
   */
  estimate: { total: number; pricingTier: PricingTier };
}

/** Stable signature of the cart lines — a new idempotency key is minted when it changes. */
const cartSignature = (lines: { productId: string; quantity: number }[]) =>
  lines
    .map((l) => `${l.productId}:${l.quantity}`)
    .sort()
    .join('|');

function toFailure(err: unknown): CheckoutFailure {
  if (!isApiError(err)) {
    return { kind: 'unknown', message: 'Something went wrong while placing your order. Please try again.' };
  }
  const e = err as ApiError;
  switch (e.code) {
    case 'out_of_stock':
      return {
        kind: 'out_of_stock',
        message: 'Some items in your cart no longer have enough stock.',
        shortages: e.details ?? [],
      };
    case 'product_unavailable':
    case 'not_found':
      return {
        kind: 'product_unavailable',
        message: 'One or more products in your cart are no longer available. Please remove them to continue.',
      };
    case 'too_many_requests':
      return {
        kind: 'rate_limited',
        retryAfter: e.retryAfter,
        message: e.retryAfter
          ? `Too many order attempts. Please try again in ${e.retryAfter} second${e.retryAfter === 1 ? '' : 's'}.`
          : 'Too many order attempts. Please wait a minute and try again.',
      };
    case 'validation_error':
    case 'payload_too_large':
      {
        const mapped = e.message ? mapServerValidation(e.message) : null;
        return mapped
          ? { kind: 'validation', message: mapped.message, field: mapped.field }
          : { kind: 'validation', message: e.message || 'Please check your details and try again.' };
      }
    case 'idempotency_conflict':
      return {
        kind: 'idempotency_conflict',
        message:
          'An earlier attempt of this checkout was already received with different details. Please contact us on WhatsApp before placing it again.',
      };
    case 'network_error':
    case 'timeout':
    case 'service_unavailable':
      return {
        kind: 'network',
        message: `${e.message} Retrying is safe — your order will not be placed twice.`,
      };
    default:
      if (e.status >= 500) {
        return {
          kind: 'network',
          message: 'The server had a problem placing your order. Retrying is safe — your order will not be placed twice.',
        };
      }
      return { kind: 'unknown', message: e.message || 'Could not place your order. Please try again.' };
  }
}

/**
 * Places real orders through `POST /orders`.
 *
 * Idempotency: one key per checkout attempt. The same key is reused for every retry of the
 * same cart (double-clicks, network failures, timeouts), so the server returns the original
 * order rather than reserving stock twice. A fresh key is minted after a successful order or
 * once the cart lines change.
 */
export function useCheckout() {
  const queryClient = useQueryClient();
  const pricing = usePricingRules();
  const attemptRef = useRef<{ key: string; cartSig: string; ambiguous: PlaceOrderPayload[] } | null>(null);
  const inFlightRef = useRef(false);

  const [isPending, setIsPending] = useState(false);
  const [failure, setFailure] = useState<CheckoutFailure | null>(null);
  const [success, setSuccess] = useState<CheckoutSuccess | null>(null);

  const refreshProducts = useCallback(
    () => queryClient.invalidateQueries({ queryKey: PRODUCTS_QUERY_KEY }),
    [queryClient]
  );

  const submit = useCallback(
    async (cartItems: CartItem[], values: CheckoutFormValues): Promise<CheckoutSuccess | null> => {
      if (inFlightRef.current) return null; // double-submit guard
      const lines = cartItems.map((i) => ({ productId: i.product.id, quantity: i.quantity }));
      const cartTotals = computeTotals(cartItems, pricing);
      const estimate = {
        total: cartTotals.total,
        pricingTier: (cartTotals.bulkUnlocked ? 'bulk' : 'standard') as PricingTier,
      };
      if (lines.length === 0) return null;
      if (lines.length > ORDER_LIMITS.maxLines) {
        setFailure({
          kind: 'validation',
          message: `An order can contain at most ${ORDER_LIMITS.maxLines} different products. Please split it into two orders.`,
        });
        return null;
      }

      const sig = cartSignature(lines);
      if (!attemptRef.current || attemptRef.current.cartSig !== sig) {
        attemptRef.current = { key: newIdempotencyKey(), cartSig: sig, ambiguous: [] };
      }
      const attempt = attemptRef.current;
      const customer = toOrderCustomer(values);
      const notes = values.notes.trim() || null;
      const payload: PlaceOrderPayload = {
        idempotencyKey: attempt.key,
        customer,
        notes,
        items: lines,
      };

      inFlightRef.current = true;
      setIsPending(true);
      setFailure(null);
      try {
        let placed: PlaceOrderResult;
        let accepted: PlaceOrderPayload = payload;
        try {
          placed = await placeOrder(payload);
        } catch (err) {
          // `idempotency_conflict` means an earlier, ambiguous attempt with this key (e.g. one that
          // timed out after the server committed it) was stored with different details. Replay
          // those exact payloads to fetch the original order instead of creating a second one.
          if (!(isApiError(err) && err.code === 'idempotency_conflict')) throw err;
          let recovered: { placed: PlaceOrderResult; payload: PlaceOrderPayload } | null = null;
          for (const previous of [...attempt.ambiguous].reverse()) {
            try {
              recovered = { placed: await placeOrder(previous), payload: previous };
              break;
            } catch (replayErr) {
              if (!(isApiError(replayErr) && replayErr.code === 'idempotency_conflict')) throw replayErr;
            }
          }
          if (!recovered) throw err;
          placed = recovered.placed;
          accepted = recovered.payload;
        }

        const result: CheckoutSuccess = {
          order: placed.order,
          orderToken: placed.orderToken,
          customer: accepted.customer,
          notes: accepted.notes,
          recoveredPreviousAttempt: accepted !== payload,
          estimate,
        };
        attemptRef.current = null; // the next checkout gets a fresh key
        setSuccess(result);
        void refreshProducts();
        return result;
      } catch (err) {
        const f = toFailure(err);
        // A network/timeout/5xx failure may still have committed server-side — remember it so a
        // retry with edited details can recover that order (see above).
        if (f.kind === 'network') attempt.ambiguous.push(payload);
        setFailure(f);
        if (f.kind === 'out_of_stock' || f.kind === 'product_unavailable') {
          void refreshProducts();
        }
        return null;
      } finally {
        inFlightRef.current = false;
        setIsPending(false);
      }
    },
    [refreshProducts, pricing]
  );

  const clearFailure = useCallback(() => setFailure(null), []);
  const resetSuccess = useCallback(() => setSuccess(null), []);

  return { submit, isPending, failure, clearFailure, success, resetSuccess };
}
