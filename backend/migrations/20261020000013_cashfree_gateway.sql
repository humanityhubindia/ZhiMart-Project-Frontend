-- Cashfree Payments replaces Razorpay as the (only) online payment gateway.
--
-- The payment columns get gateway-neutral names (they said "razorpay") and
-- looser id formats: Cashfree order ids are OURS (`{order_number}_{12 hex}`,
-- see `payments::cashfree::gateway_order_id`), payment ids are Cashfree's
-- numeric `cf_payment_id` stored as a digit string, and refund ids are
-- Cashfree's numeric `cf_refund_id`. Historical Razorpay ids
-- (`order_…`, `pay_…`, `rfnd_…`) still satisfy the new checks, so every
-- existing row stays valid and keeps its `payment_method = 'razorpay'`.
--
-- Unpaid orders that were waiting for a Razorpay payment are switched to
-- Cashfree (their Razorpay order id is dropped; the next "Pay" creates a
-- Cashfree order). Paid Razorpay orders are left alone: refund those from the
-- Razorpay Dashboard (the API refuses them with a hint).
--
-- 20261014000012_google_sheets_sync.sql's trigger function does not name any
-- payment column (it fires on `OLD.* IS DISTINCT FROM NEW.*`), so nothing there
-- needs recreating.

-- orders ---------------------------------------------------------------------

ALTER TABLE orders RENAME COLUMN razorpay_order_id TO gateway_order_id;
ALTER TABLE orders RENAME COLUMN razorpay_payment_id TO gateway_payment_id;

ALTER TABLE orders DROP CONSTRAINT orders_rzp_order_id_format;
ALTER TABLE orders DROP CONSTRAINT orders_rzp_payment_id_format;
ALTER TABLE orders
    ADD CONSTRAINT orders_gateway_order_id_format
        CHECK (gateway_order_id IS NULL OR gateway_order_id ~ '^[A-Za-z0-9_-]{3,64}$'),
    ADD CONSTRAINT orders_gateway_payment_id_format
        CHECK (gateway_payment_id IS NULL OR gateway_payment_id ~ '^[A-Za-z0-9_-]{1,64}$');

ALTER INDEX orders_razorpay_order_id_key RENAME TO orders_gateway_order_id_key;
ALTER INDEX orders_razorpay_payment_id_key RENAME TO orders_gateway_payment_id_key;

ALTER TABLE orders DROP CONSTRAINT orders_payment_method;
ALTER TABLE orders ADD CONSTRAINT orders_payment_method CHECK (
    payment_method IN ('cashfree', 'razorpay', 'pay_later', 'upi', 'card', 'netbanking', 'cod', 'bank_transfer')
);

-- Orders still awaiting an online payment move to Cashfree.
UPDATE orders
SET payment_method = 'cashfree',
    gateway_order_id = NULL,
    version = version + 1,
    updated_at = now()
WHERE status = 'pending_payment'
  AND gateway_payment_id IS NULL
  AND payment_method = 'razorpay';

-- refunds --------------------------------------------------------------------

ALTER TABLE refunds RENAME COLUMN razorpay_payment_id TO gateway_payment_id;
ALTER TABLE refunds RENAME COLUMN razorpay_refund_id TO gateway_refund_id;

ALTER TABLE refunds DROP CONSTRAINT refunds_rzp_id_format;
ALTER TABLE refunds ADD CONSTRAINT refunds_gateway_refund_id_format
    CHECK (gateway_refund_id IS NULL OR gateway_refund_id ~ '^[A-Za-z0-9_-]{1,64}$');

ALTER INDEX refunds_razorpay_refund_id_key RENAME TO refunds_gateway_refund_id_key;

-- payment_events -------------------------------------------------------------

ALTER TABLE payment_events RENAME COLUMN razorpay_order_id TO gateway_order_id;
ALTER TABLE payment_events RENAME COLUMN razorpay_payment_id TO gateway_payment_id;
