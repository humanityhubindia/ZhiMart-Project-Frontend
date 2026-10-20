import React, { useState } from 'react';
import { Check, Copy, CreditCard, ReceiptText, RotateCcw, Undo2, XCircle } from 'lucide-react';
import { usePaymentEvents } from '../../hooks/admin/adminQueries';
import type { Order, PaymentAction } from '../../types/admin';
import {
  formatDateTime,
  formatINR,
  gatewayDashboardName,
  paymentMethodSummary,
  refundableAmount,
} from './format';
import { availablePaymentActions, canRefundOnline, legacyGatewayRefundHint, PAYMENT_ACTION_LABEL } from './OrderActionDialogs';
import { useToast } from './Toast';
import { Button, ErrorCard, PaymentStatusBadge, Skeleton } from './ui';

export function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  const toast = useToast();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error('Could not copy — select the text and copy it manually.');
    }
  };
  return (
    <button
      type="button"
      onClick={() => void copy()}
      aria-label={`Copy ${label}`}
      title={`Copy ${label}`}
      className="p-1 rounded-md text-slate-400 hover:text-slate-900 hover:bg-slate-100 shrink-0"
    >
      {copied ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
    </button>
  );
}

/**
 * Monospace gateway id with a copy button. The Cashfree Merchant Dashboard has no stable deep
 * links, so the tooltip says where to paste it instead of linking.
 */
export function GatewayId({ id, label, dashboard }: { id: string; label: string; dashboard: string }) {
  return (
    <span className="inline-flex items-center gap-0.5 max-w-full">
      <span className="font-mono text-[11px] truncate" title={`${id} — search it in the ${dashboard}`}>
        {id}
      </span>
      <CopyButton value={id} label={label} />
    </span>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between items-center gap-3 text-xs py-1 min-w-0">
      <dt className="text-slate-500 font-bold shrink-0">{label}</dt>
      <dd className="font-extrabold text-slate-900 text-right min-w-0 flex justify-end">{children}</dd>
    </div>
  );
}

export function PaymentPanel({
  order,
  onRefund,
  onManualAction,
}: {
  order: Order;
  onRefund: () => void;
  onManualAction: (action: PaymentAction) => void;
}) {
  const p = order.payment;
  const refunded = p.amountRefunded ?? 0;
  const manual = availablePaymentActions(order);
  return (
    <section className="bg-white rounded-2xl border border-slate-200 p-4 sm:p-5">
      <h3 className="text-[11px] font-extrabold uppercase tracking-wider text-slate-400 mb-3 flex items-center gap-1.5">
        <CreditCard className="w-3.5 h-3.5" /> Payment
      </h3>
      <dl>
        <Row label="Status">
          <PaymentStatusBadge status={p.status} />
        </Row>
        <Row label="Method">{paymentMethodSummary(p)}</Row>
        <Row label="Amount">{formatINR(order.totalAmount)}</Row>
        {p.paidAt && <Row label="Paid">{formatDateTime(p.paidAt)}</Row>}
        {p.gatewayOrderId && (
          <Row label="Gateway order">
            <GatewayId id={p.gatewayOrderId} label="gateway order id" dashboard={gatewayDashboardName(p.method)} />
          </Row>
        )}
        {p.gatewayPaymentId && (
          <Row label="Gateway payment">
            <GatewayId id={p.gatewayPaymentId} label="gateway payment id" dashboard={gatewayDashboardName(p.method)} />
          </Row>
        )}
        {p.reference && (
          <Row label="Reference">
            <span className="font-mono">{p.reference}</span>
          </Row>
        )}
        {(refunded > 0 || p.status === 'refunded') && (
          <>
            <Row label="Refunded">
              <span className="text-rose-700">{formatINR(refunded || order.totalAmount)}</span>
            </Row>
            {p.status !== 'refunded' && <Row label="Refundable">{formatINR(refundableAmount(order))}</Row>}
            {p.refundedAt && <Row label="Refunded at">{formatDateTime(p.refundedAt)}</Row>}
          </>
        )}
      </dl>
      {manual.includes('mark_paid') && (
        <p className="mt-3 p-2.5 rounded-xl bg-amber-50 border border-amber-200 text-[11px] font-bold text-amber-900">
          {order.payment.method === 'pay_later'
            ? 'Pay-later order — collect the payment (UPI, bank transfer or cash), then record it here with the UTR / reference.'
            : 'Payment not yet received — record it once collected.'}
        </p>
      )}
      {legacyGatewayRefundHint(order) && (
        <p className="mt-3 p-2.5 rounded-xl bg-slate-50 border border-slate-200 text-[11px] font-bold text-slate-700">
          {legacyGatewayRefundHint(order)}
        </p>
      )}
      {(canRefundOnline(order) || manual.length > 0) && (
        <div className="mt-3 pt-3 border-t border-slate-100 flex flex-wrap gap-2">
          {canRefundOnline(order) && (
            <Button size="sm" variant="dangerOutline" icon={RotateCcw} onClick={onRefund}>
              Refund…
            </Button>
          )}
          {manual.map((a) => (
            <Button
              key={a}
              size="sm"
              variant={a === 'mark_paid' ? 'success' : 'dangerOutline'}
              icon={a === 'mark_paid' ? CreditCard : a === 'mark_refunded' ? Undo2 : XCircle}
              onClick={() => onManualAction(a)}
            >
              {PAYMENT_ACTION_LABEL[a]}
            </Button>
          ))}
        </div>
      )}
    </section>
  );
}

const REFUND_STATUS_CLASS: Record<string, string> = {
  processed: 'bg-emerald-50 text-emerald-800 border-emerald-200',
  pending: 'bg-amber-50 text-amber-800 border-amber-200',
  failed: 'bg-rose-50 text-rose-700 border-rose-200',
};

export function PaymentHistoryPanel({ orderId }: { orderId: string }) {
  const history = usePaymentEvents(orderId);
  return (
    <section className="bg-white rounded-2xl border border-slate-200 p-4 sm:p-5">
      <h3 className="text-[11px] font-extrabold uppercase tracking-wider text-slate-400 mb-3 flex items-center gap-1.5">
        <ReceiptText className="w-3.5 h-3.5" /> Payment history
      </h3>
      {history.isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-8" />
          <Skeleton className="h-8" />
        </div>
      ) : history.isError ? (
        <ErrorCard compact title="Could not load payment history" error={history.error} onRetry={() => void history.refetch()} retrying={history.isFetching} />
      ) : history.data.events.length === 0 && history.data.refunds.length === 0 ? (
        <p className="text-xs text-slate-500">No gateway events or refunds recorded for this order.</p>
      ) : (
        <div className="space-y-4">
          {history.data.refunds.length > 0 && (
            <div>
              <p className="text-[11px] font-extrabold text-slate-700 mb-1.5">Refunds</p>
              <ul className="divide-y divide-slate-100">
                {history.data.refunds.map((r) => (
                  <li key={r.id} className="py-2 text-xs">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="font-black text-slate-900 tabular-nums">{formatINR(r.amount)}</span>
                      <span
                        className={`inline-flex px-2 py-0.5 rounded-lg text-[11px] font-extrabold border capitalize ${
                          REFUND_STATUS_CLASS[r.status] ?? 'bg-slate-100 text-slate-700 border-slate-300'
                        }`}
                      >
                        {r.status}
                      </span>
                    </div>
                    <p className="text-slate-600 mt-0.5">{r.reason}</p>
                    {r.failureReason && <p className="text-rose-700 font-bold mt-0.5">{r.failureReason}</p>}
                    <p className="text-[11px] text-slate-400 mt-0.5 flex flex-wrap items-center gap-x-1">
                      {formatDateTime(r.createdAt)} · {r.adminEmail ? `by ${r.adminEmail}` : 'automatic'}
                      {r.gatewayRefundId && (
                        <>
                          {' · '}
                          <span className="font-mono">{r.gatewayRefundId}</span>
                          <CopyButton value={r.gatewayRefundId} label="gateway refund id" />
                        </>
                      )}
                    </p>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {history.data.events.length > 0 && (
            <div>
              <p className="text-[11px] font-extrabold text-slate-700 mb-1.5">Gateway events</p>
              <ol className="divide-y divide-slate-100">
                {history.data.events.map((e) => (
                  <li key={e.id} className="py-2 text-xs flex flex-wrap items-start justify-between gap-x-3 gap-y-0.5">
                    <span className="font-mono font-bold text-slate-900">{e.event}</span>
                    <span className="text-[11px] text-slate-400">{formatDateTime(e.receivedAt)}</span>
                    <span className="basis-full text-[11px] text-slate-600">{e.outcome ?? 'Not processed yet'}</span>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
