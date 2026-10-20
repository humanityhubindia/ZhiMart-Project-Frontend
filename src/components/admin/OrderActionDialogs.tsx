import React, { useEffect, useId, useState } from 'react';
import { Ban, CreditCard, RotateCcw, Truck } from 'lucide-react';
import { isApiError } from '../../lib/api';
import {
  useCancelOrder,
  useRecordPayment,
  useRefundOrder,
  useUpdateOrderShipping,
  useUpdateOrderStatus,
} from '../../hooks/admin/adminMutations';
import type { Order, OrderStatus, PaymentAction, PaymentMethod } from '../../types/admin';
import { MANUAL_PAYMENT_METHODS } from '../../types/admin';
import {
  formatCount,
  formatINR,
  isOnlinePaymentMethod,
  nextOrderStatuses,
  ORDER_CONFLICT_MESSAGE,
  ORDER_STATUS_META,
  PAYMENT_METHOD_LABEL,
  refundableAmount,
  requiresShipping,
} from './format';
import { useToast } from './Toast';
import { Button, FieldError, FormError, inputClass, inputErrorClass, labelClass, Modal } from './ui';

export const TRANSPORT_PARTNER_SUGGESTIONS = [
  'VRL Logistics',
  'ABT Parcel Service',
  'ARC Transports',
  'KPN Speed Parcel',
  'Professional Couriers',
];

/** Shared error handling: conflicts close the dialog (data is refetched), others stay inline. */
function useOrderErrorHandler(onClose: () => void) {
  const toast = useToast();
  return (err: unknown, setError: (msg: string) => void) => {
    if (isApiError(err) && (err.code === 'version_conflict' || err.code === 'not_found')) {
      toast.info(err.code === 'not_found' ? 'This order no longer exists.' : ORDER_CONFLICT_MESSAGE);
      onClose();
      return;
    }
    setError(isApiError(err) ? err.message : 'Something went wrong. Please try again.');
  };
}

function TransportFields({
  partner,
  docket,
  setPartner,
  setDocket,
  required,
  errors,
}: {
  partner: string;
  docket: string;
  setPartner: (v: string) => void;
  setDocket: (v: string) => void;
  required: boolean;
  errors: { partner?: string; docket?: string };
}) {
  const listId = useId();
  const pId = useId();
  const dId = useId();
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <div>
        <label htmlFor={pId} className={labelClass}>
          Transport partner{required && <span className="text-rose-500"> *</span>}
        </label>
        <input
          id={pId}
          list={listId}
          value={partner}
          maxLength={80}
          onChange={(e) => setPartner(e.target.value)}
          placeholder="e.g. VRL Logistics"
          aria-invalid={!!errors.partner}
          aria-describedby={errors.partner ? `${pId}-err` : undefined}
          className={`${inputClass} ${errors.partner ? inputErrorClass : ''}`}
        />
        <datalist id={listId}>
          {TRANSPORT_PARTNER_SUGGESTIONS.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
        <FieldError id={`${pId}-err`} message={errors.partner} />
      </div>
      <div>
        <label htmlFor={dId} className={labelClass}>
          Docket / LR number{required && <span className="text-rose-500"> *</span>}
        </label>
        <input
          id={dId}
          value={docket}
          maxLength={80}
          onChange={(e) => setDocket(e.target.value)}
          placeholder="Lorry receipt number"
          aria-invalid={!!errors.docket}
          aria-describedby={errors.docket ? `${dId}-err` : undefined}
          className={`${inputClass} font-mono ${errors.docket ? inputErrorClass : ''}`}
        />
        <FieldError id={`${dId}-err`} message={errors.docket} />
      </div>
    </div>
  );
}

function validateTransport(partner: string, docket: string, required: boolean) {
  const errors: { partner?: string; docket?: string } = {};
  if (required && !partner.trim()) errors.partner = 'Transport partner is required for dispatch.';
  if (required && !docket.trim()) errors.docket = 'Docket number is required for dispatch.';
  if (partner.trim().length > 80) errors.partner = 'At most 80 characters.';
  if (docket.trim().length > 80) errors.docket = 'At most 80 characters.';
  return errors;
}

// ---------------------------------------------------------------------------
// Advance status
// ---------------------------------------------------------------------------

export function OrderStatusDialog({ order, open, onClose }: { order: Order; open: boolean; onClose: () => void }) {
  const options = nextOrderStatuses(order.status);
  const [target, setTarget] = useState<OrderStatus | ''>(options[0] ?? '');
  const [partner, setPartner] = useState(order.shipping.transportPartner ?? '');
  const [docket, setDocket] = useState(order.shipping.docketNumber ?? '');
  const [errors, setErrors] = useState<{ partner?: string; docket?: string }>({});
  const [formError, setFormError] = useState<string | null>(null);
  const mutation = useUpdateOrderStatus();
  const toast = useToast();
  const handleError = useOrderErrorHandler(onClose);
  const selectId = useId();

  useEffect(() => {
    if (!open) return;
    setTarget(nextOrderStatuses(order.status)[0] ?? '');
    setPartner(order.shipping.transportPartner ?? '');
    setDocket(order.shipping.docketNumber ?? '');
    setErrors({});
    setFormError(null);
    // Reset only when (re)opened.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const needsShipping = target !== '' && requiresShipping(target);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!target) return;
    const errs = validateTransport(partner, docket, needsShipping);
    setErrors(errs);
    if (Object.keys(errs).length) return;
    setFormError(null);
    mutation.mutate(
      {
        id: order.id,
        input: {
          status: target,
          expectedVersion: order.version,
          ...(needsShipping ? { transportPartner: partner.trim(), docketNumber: docket.trim() } : {}),
        },
      },
      {
        onSuccess: (updated) => {
          toast.success(`${updated.orderNumber} marked ${ORDER_STATUS_META[updated.status].label.toLowerCase()}.`);
          onClose();
        },
        onError: (err) => handleError(err, setFormError),
      }
    );
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Update fulfilment status"
      description={`${order.orderNumber} is currently ${ORDER_STATUS_META[order.status].label}.`}
      icon={Truck}
      dismissable={!mutation.isPending}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" form="order-status-form" loading={mutation.isPending} disabled={!target}>
            Update status
          </Button>
        </>
      }
    >
      {options.length === 0 ? (
        <p className="text-xs text-slate-600">This order is already in its final state.</p>
      ) : (
        <form id="order-status-form" onSubmit={submit} className="space-y-4" noValidate>
          <div>
            <label htmlFor={selectId} className={labelClass}>
              New status
            </label>
            <select
              id={selectId}
              value={target}
              onChange={(e) => setTarget(e.target.value as OrderStatus)}
              className={inputClass}
            >
              {options.map((s) => (
                <option key={s} value={s}>
                  {ORDER_STATUS_META[s].label}
                </option>
              ))}
            </select>
            <p className="text-[11px] text-slate-500 mt-1">
              Orders only move forward. Skipping ahead (e.g. straight to Delivered) is allowed.
            </p>
          </div>
          {needsShipping && (
            <TransportFields
              partner={partner}
              docket={docket}
              setPartner={setPartner}
              setDocket={setDocket}
              required
              errors={errors}
            />
          )}
          <FormError message={formError} />
        </form>
      )}
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Edit shipping
// ---------------------------------------------------------------------------

export function OrderShippingDialog({ order, open, onClose }: { order: Order; open: boolean; onClose: () => void }) {
  const [partner, setPartner] = useState(order.shipping.transportPartner ?? '');
  const [docket, setDocket] = useState(order.shipping.docketNumber ?? '');
  const [errors, setErrors] = useState<{ partner?: string; docket?: string }>({});
  const [formError, setFormError] = useState<string | null>(null);
  const mutation = useUpdateOrderShipping();
  const toast = useToast();
  const handleError = useOrderErrorHandler(onClose);
  const required = requiresShipping(order.status);

  useEffect(() => {
    if (!open) return;
    setPartner(order.shipping.transportPartner ?? '');
    setDocket(order.shipping.docketNumber ?? '');
    setErrors({});
    setFormError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const errs = validateTransport(partner, docket, required);
    setErrors(errs);
    if (Object.keys(errs).length) return;
    setFormError(null);
    mutation.mutate(
      {
        id: order.id,
        input: {
          transportPartner: partner.trim() || null,
          docketNumber: docket.trim() || null,
          expectedVersion: order.version,
        },
      },
      {
        onSuccess: (updated) => {
          toast.success(`Shipping details saved for ${updated.orderNumber}.`);
          onClose();
        },
        onError: (err) => handleError(err, setFormError),
      }
    );
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Transport & docket"
      description={`${order.orderNumber} · ${order.customer.name}, ${order.customer.city}`}
      icon={Truck}
      dismissable={!mutation.isPending}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" form="order-shipping-form" loading={mutation.isPending}>
            Save shipping
          </Button>
        </>
      }
    >
      <form id="order-shipping-form" onSubmit={submit} className="space-y-4" noValidate>
        <TransportFields
          partner={partner}
          docket={docket}
          setPartner={setPartner}
          setDocket={setDocket}
          required={required}
          errors={errors}
        />
        {!required && (
          <p className="text-[11px] text-slate-500">
            Optional before dispatch — you can also enter these when marking the order dispatched.
          </p>
        )}
        <FormError message={formError} />
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

export function OrderCancelDialog({ order, open, onClose }: { order: Order; open: boolean; onClose: () => void }) {
  const [reason, setReason] = useState('');
  const [reasonError, setReasonError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const mutation = useCancelOrder();
  const toast = useToast();
  const handleError = useOrderErrorHandler(onClose);
  const reasonId = useId();

  useEffect(() => {
    if (!open) return;
    setReason('');
    setReasonError(null);
    setFormError(null);
  }, [open]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const r = reason.replace(/\s+/g, ' ').trim();
    if (!r) return setReasonError('Please give a reason for the cancellation.');
    if (r.length > 300) return setReasonError('At most 300 characters.');
    setReasonError(null);
    setFormError(null);
    mutation.mutate(
      { id: order.id, input: { reason: r, expectedVersion: order.version } },
      {
        onSuccess: (updated) => {
          toast.success(`${updated.orderNumber} cancelled — stock restored for ${updated.items.length} product(s).`);
          onClose();
        },
        onError: (err) => handleError(err, setFormError),
      }
    );
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`Cancel ${order.orderNumber}?`}
      icon={Ban}
      size="md"
      dismissable={!mutation.isPending}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={mutation.isPending}>
            Keep order
          </Button>
          <Button variant="danger" type="submit" form="order-cancel-form" loading={mutation.isPending}>
            Cancel order
          </Button>
        </>
      }
    >
      <form id="order-cancel-form" onSubmit={submit} className="space-y-4" noValidate>
        <div className="p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-xs text-amber-900 leading-relaxed">
          <p className="font-extrabold">This cannot be undone.</p>
          <p className="mt-1">
            The reserved stock for every line is returned to inventory immediately ({formatCount(order.itemCount)} unit
            {order.itemCount === 1 ? '' : 's'} across {order.items.length} product{order.items.length === 1 ? '' : 's'}).
            {order.status === 'pending_payment'
              ? ' The customer will no longer be able to pay for this order.'
              : ` Order value ${formatINR(order.totalAmount)} is removed from revenue.`}
          </p>
          {order.payment.status === 'paid' && (
            <p className="mt-1 font-bold">
              This order is already paid — cancelling does not return the money. Issue a refund from the Payment section.
            </p>
          )}
        </div>
        <ul className="text-xs text-slate-700 space-y-1 max-h-32 overflow-y-auto">
          {order.items.map((it) => (
            <li key={it.productId} className="flex justify-between gap-3">
              <span className="truncate">{it.productName}</span>
              <span className="font-bold tabular-nums">+{formatCount(it.quantity)}</span>
            </li>
          ))}
        </ul>
        <div>
          <label htmlFor={reasonId} className={labelClass}>
            Reason <span className="text-rose-500">*</span>
          </label>
          <textarea
            id={reasonId}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            maxLength={300}
            placeholder="e.g. Customer requested cancellation on call"
            aria-invalid={!!reasonError}
            aria-describedby={reasonError ? `${reasonId}-err` : undefined}
            className={`${inputClass} resize-none ${reasonError ? inputErrorClass : ''}`}
          />
          <div className="flex justify-between">
            <FieldError id={`${reasonId}-err`} message={reasonError} />
            <span className="text-[10px] text-slate-400 ml-auto mt-1 tabular-nums">{reason.trim().length}/300</span>
          </div>
        </div>
        <FormError message={formError} />
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Payment
// ---------------------------------------------------------------------------

/**
 * Manual payment actions — only for pay-later/manual methods. Gateway payments (Cashfree, and
 * legacy Razorpay) are settled by the gateway and reversed with refunds; the server refuses these too.
 */
export function availablePaymentActions(order: Order): PaymentAction[] {
  if (isOnlinePaymentMethod(order.payment.method) || order.status === 'pending_payment') return [];
  switch (order.payment.status) {
    case 'pending':
      // A cancelled order cannot be marked paid or failed.
      return order.status === 'cancelled' ? [] : ['mark_paid', 'mark_failed'];
    case 'failed':
      return order.status === 'cancelled' ? [] : ['mark_paid'];
    case 'paid':
      return ['mark_refunded'];
    default:
      return [];
  }
}

export const PAYMENT_ACTION_LABEL: Record<PaymentAction, string> = {
  mark_paid: 'Record payment',
  mark_failed: 'Mark failed',
  mark_refunded: 'Record refund',
};

const SETTLEMENT_METHODS = MANUAL_PAYMENT_METHODS;

export function OrderPaymentDialog({
  order,
  action,
  onClose,
}: {
  order: Order;
  action: PaymentAction | null;
  onClose: () => void;
}) {
  const open = action !== null;
  const [method, setMethod] = useState<PaymentMethod>('upi');
  const [reference, setReference] = useState('');
  const [refError, setRefError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const mutation = useRecordPayment();
  const toast = useToast();
  const handleError = useOrderErrorHandler(onClose);
  const methodId = useId();
  const refId = useId();

  useEffect(() => {
    if (!open) return;
    const current = order.payment.method;
    setMethod((MANUAL_PAYMENT_METHODS as readonly string[]).includes(current) ? current : 'upi');
    setReference(order.payment.reference ?? '');
    setRefError(null);
    setFormError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!action) return null;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const ref = reference.trim();
    if (action === 'mark_paid' && ref.length > 120) return setRefError('At most 120 characters.');
    setRefError(null);
    setFormError(null);
    mutation.mutate(
      {
        id: order.id,
        input:
          action === 'mark_paid'
            ? { action, method, reference: ref || null, expectedVersion: order.version }
            : { action, expectedVersion: order.version },
      },
      {
        onSuccess: (updated) => {
          const verb = action === 'mark_paid' ? 'marked paid' : action === 'mark_failed' ? 'marked failed' : 'marked refunded';
          toast.success(`Payment for ${updated.orderNumber} ${verb}.`);
          onClose();
        },
        onError: (err) => handleError(err, setFormError),
      }
    );
  };

  const title =
    action === 'mark_paid'
      ? `Record payment · ${order.orderNumber}`
      : action === 'mark_failed'
        ? `Mark payment failed · ${order.orderNumber}`
        : `Record refund · ${order.orderNumber}`;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      description={`Order total ${formatINR(order.totalAmount)} · ${order.customer.name}`}
      icon={CreditCard}
      dismissable={!mutation.isPending}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={mutation.isPending}>
            Cancel
          </Button>
          <Button
            variant={action === 'mark_paid' ? 'success' : 'danger'}
            type="submit"
            form="order-payment-form"
            loading={mutation.isPending}
          >
            {PAYMENT_ACTION_LABEL[action]}
          </Button>
        </>
      }
    >
      <form id="order-payment-form" onSubmit={submit} className="space-y-4" noValidate>
        {action === 'mark_paid' && (
          <>
            <p className="text-xs text-slate-600 leading-relaxed">
              Confirm that {formatINR(order.totalAmount)} has been received. No online gateway is connected — this
              records a payment collected outside the site.
            </p>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div>
                <label htmlFor={methodId} className={labelClass}>
                  Method <span className="text-rose-500">*</span>
                </label>
                <select
                  id={methodId}
                  value={method}
                  onChange={(e) => setMethod(e.target.value as PaymentMethod)}
                  className={inputClass}
                >
                  {SETTLEMENT_METHODS.map((m) => (
                    <option key={m} value={m}>
                      {PAYMENT_METHOD_LABEL[m]}
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <label htmlFor={refId} className={labelClass}>
                  Reference (UTR / txn ID)
                </label>
                <input
                  id={refId}
                  value={reference}
                  maxLength={120}
                  onChange={(e) => setReference(e.target.value)}
                  placeholder="Optional"
                  aria-invalid={!!refError}
                  aria-describedby={refError ? `${refId}-err` : undefined}
                  className={`${inputClass} font-mono ${refError ? inputErrorClass : ''}`}
                />
                <FieldError id={`${refId}-err`} message={refError} />
              </div>
            </div>
          </>
        )}
        {action === 'mark_failed' && (
          <p className="text-xs text-slate-600 leading-relaxed">
            Mark this payment as failed. The order stays open, and you can still record a successful payment later.
          </p>
        )}
        {action === 'mark_refunded' && (
          <div className="p-3.5 rounded-xl bg-rose-50 border border-rose-200 text-xs text-rose-900 leading-relaxed">
            <p className="font-extrabold">Record a refund of {formatINR(order.totalAmount)}.</p>
            <p className="mt-1">
              This only records the refund — send the money back through the original channel
              {order.payment.reference ? ` (ref ${order.payment.reference})` : ''}. The amount is removed from revenue
              and cannot be marked paid again.
            </p>
          </div>
        )}
        <FormError message={formError} />
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Cashfree refund
// ---------------------------------------------------------------------------

const MONEY_RE = /^\d+(\.\d{1,2})?$/;

/**
 * Refundable through the API: paid Cashfree orders only. Legacy Razorpay payments must be refunded
 * from the Razorpay Dashboard (the server answers 409 for them) — see `legacyGatewayRefundHint`.
 */
export function canRefundOnline(order: Order): boolean {
  return (
    order.payment.method === 'cashfree' &&
    order.payment.status === 'paid' &&
    !!order.payment.gatewayPaymentId &&
    refundableAmount(order) > 0
  );
}

/** Why a paid legacy Razorpay order has no Refund button (null when not applicable). */
export function legacyGatewayRefundHint(order: Order): string | null {
  return order.payment.method === 'razorpay' && order.payment.status === 'paid' && refundableAmount(order) > 0
    ? 'Paid through Razorpay before the switch to Cashfree — refund it from the Razorpay Dashboard.'
    : null;
}

export function OrderRefundDialog({ order, open, onClose }: { order: Order; open: boolean; onClose: () => void }) {
  const remaining = refundableAmount(order);
  const [mode, setMode] = useState<'full' | 'partial'>('full');
  const [amount, setAmount] = useState('');
  const [reason, setReason] = useState('');
  const [errors, setErrors] = useState<{ amount?: string; reason?: string }>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [step, setStep] = useState<'form' | 'confirm'>('form');
  const mutation = useRefundOrder();
  const toast = useToast();
  const handleError = useOrderErrorHandler(onClose);
  const amountId = useId();
  const reasonId = useId();

  useEffect(() => {
    if (!open) return;
    setMode('full');
    setAmount('');
    setReason('');
    setErrors({});
    setFormError(null);
    setStep('form');
    mutation.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const refundValue = mode === 'full' ? remaining : Number(amount);
  const cleanReason = reason.replace(/\s+/g, ' ').trim();

  const review = (e: React.FormEvent) => {
    e.preventDefault();
    const errs: { amount?: string; reason?: string } = {};
    if (mode === 'partial') {
      const v = amount.trim();
      if (!v) errs.amount = 'Enter the amount to refund.';
      else if (!MONEY_RE.test(v) || Number(v) <= 0) errs.amount = 'Enter an amount above ₹0 with at most 2 decimals.';
      else if (Number(v) > remaining) errs.amount = `At most ${formatINR(remaining)} can still be refunded.`;
    }
    if (!cleanReason) errs.reason = 'A reason is required.';
    else if (cleanReason.length > 300) errs.reason = 'At most 300 characters.';
    setErrors(errs);
    if (Object.keys(errs).length) return;
    setFormError(null);
    setStep('confirm');
  };

  const submit = () => {
    setFormError(null);
    mutation.mutate(
      {
        id: order.id,
        input: {
          amount: mode === 'full' ? null : Number(amount.trim()),
          reason: cleanReason,
          expectedVersion: order.version,
        },
      },
      {
        onSuccess: (updated) => {
          const fully = updated.payment.status === 'refunded';
          toast.success(
            `Refund of ${formatINR(refundValue)} for ${updated.orderNumber} sent to Cashfree${fully ? ' — fully refunded' : ''}.`
          );
          onClose();
        },
        onError: (err) => {
          if (isApiError(err) && err.code === 'payment_gateway_error') {
            setStep('form');
            setFormError(
              'Cashfree did not confirm the refund (gateway error or timeout). Check Payment history before retrying — a refund may already be pending.'
            );
            return;
          }
          setStep('form');
          handleError(err, setFormError);
        },
      }
    );
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      icon={RotateCcw}
      title={`Refund · ${order.orderNumber}`}
      description={`Paid ${formatINR(order.totalAmount)}${
        order.payment.amountRefunded > 0 ? ` · already refunded ${formatINR(order.payment.amountRefunded)}` : ''
      } · refundable ${formatINR(remaining)}`}
      dismissable={!mutation.isPending}
      footer={
        step === 'form' ? (
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button variant="danger" type="submit" form="order-refund-form" disabled={remaining <= 0}>
              Review refund
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={() => setStep('form')} disabled={mutation.isPending}>
              Back
            </Button>
            <Button variant="danger" onClick={submit} loading={mutation.isPending} data-autofocus>
              Refund {formatINR(refundValue)}
            </Button>
          </>
        )
      }
    >
      {step === 'form' ? (
        <form id="order-refund-form" onSubmit={review} noValidate className="space-y-4">
          <div role="radiogroup" aria-label="Refund amount" className="grid grid-cols-2 gap-2">
            {(['full', 'partial'] as const).map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={mode === m}
                onClick={() => {
                  setMode(m);
                  setErrors((x) => ({ ...x, amount: undefined }));
                }}
                className={`px-3 py-2.5 rounded-xl text-xs font-bold border transition-all ${
                  mode === m ? 'bg-slate-950 text-white border-slate-950' : 'bg-white text-slate-700 border-slate-200 hover:bg-slate-50'
                }`}
              >
                {m === 'full' ? `Full remaining (${formatINR(remaining)})` : 'Partial amount'}
              </button>
            ))}
          </div>
          {mode === 'partial' && (
            <div>
              <label htmlFor={amountId} className={labelClass}>
                Amount (₹) <span className="text-rose-500">*</span>
              </label>
              <div className="relative">
                <span className="absolute left-3.5 top-1/2 -translate-y-1/2 text-xs font-bold text-slate-400" aria-hidden>
                  ₹
                </span>
                <input
                  id={amountId}
                  inputMode="decimal"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value.replace(/[^\d.]/g, ''))}
                  placeholder={String(remaining)}
                  aria-invalid={!!errors.amount}
                  aria-describedby={errors.amount ? `${amountId}-err` : undefined}
                  className={`${inputClass} pl-7 tabular-nums ${errors.amount ? inputErrorClass : ''}`}
                />
              </div>
              <FieldError id={`${amountId}-err`} message={errors.amount} />
            </div>
          )}
          <div>
            <label htmlFor={reasonId} className={labelClass}>
              Reason <span className="text-rose-500">*</span>
            </label>
            <input
              id={reasonId}
              value={reason}
              maxLength={300}
              onChange={(e) => setReason(e.target.value)}
              placeholder="e.g. Item out of stock after payment"
              aria-invalid={!!errors.reason}
              aria-describedby={errors.reason ? `${reasonId}-err` : undefined}
              className={`${inputClass} ${errors.reason ? inputErrorClass : ''}`}
            />
            <FieldError id={`${reasonId}-err`} message={errors.reason} />
          </div>
          <p className="text-[11px] text-slate-500">
            The refund is issued through Cashfree to the customer&apos;s original payment method
            {order.payment.gatewayPaymentId ? ` (payment ${order.payment.gatewayPaymentId})` : ''}. Banks usually take
            5–7 working days.
          </p>
          <FormError message={formError} />
        </form>
      ) : (
        <div className="space-y-3">
          <div className="p-4 rounded-2xl bg-rose-50 border border-rose-200 text-rose-900 text-xs leading-relaxed">
            <p className="text-sm font-black">
              Refund {formatINR(refundValue)} to {order.customer.name}?
            </p>
            <p className="mt-1">
              {refundValue >= remaining
                ? 'This refunds everything that remains; the payment will be marked refunded.'
                : `A partial refund — ${formatINR(Math.round((remaining - refundValue) * 100) / 100)} will remain refundable.`}{' '}
              Money sent back through Cashfree cannot be recalled.
            </p>
            <p className="mt-2 text-rose-800">
              <span className="font-bold">Reason:</span> {cleanReason}
            </p>
          </div>
          <FormError message={formError} />
        </div>
      )}
    </Modal>
  );
}
