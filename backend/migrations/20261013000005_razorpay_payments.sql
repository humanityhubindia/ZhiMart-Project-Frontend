-- Razorpay online payments for guest checkout.
--
-- Lifecycle: an order is created in `pending_payment` with its stock already
-- reserved and `payment_expires_at` set. A verified payment (checkout signature
-- verified server-side, or the signed webhook) moves it to `pending_pack` /
-- payment `paid`. Unpaid orders past `payment_expires_at` are cancelled and
-- restocked by a background sweep. A payment that arrives after expiry either
-- re-reserves stock ("revived") or is refunded automatically.

ALTER TABLE orders DROP CONSTRAINT orders_status;
ALTER TABLE orders ADD CONSTRAINT orders_status CHECK (
    status IN ('pending_payment', 'pending_pack', 'dispatched', 'in_transit', 'out_for_delivery', 'delivered', 'cancelled')
);

ALTER TABLE orders DROP CONSTRAINT orders_payment_method;
ALTER TABLE orders ADD CONSTRAINT orders_payment_method CHECK (
    payment_method IN ('razorpay', 'pay_later', 'upi', 'card', 'netbanking', 'cod', 'bank_transfer')
);

ALTER TABLE orders
    -- Placed while PAYMENT_TEST_MODE=true (charged ₹1/unit). Kept forever so test
    -- orders can be excluded from real revenue.
    ADD COLUMN is_test                BOOLEAN     NOT NULL DEFAULT FALSE,
    ADD COLUMN payment_expires_at     TIMESTAMPTZ,
    ADD COLUMN razorpay_order_id      TEXT,
    ADD COLUMN razorpay_payment_id    TEXT,
    -- Instrument reported by Razorpay for the captured payment: upi | card |
    -- netbanking | wallet | emi | paylater | bank_transfer.
    ADD COLUMN payment_instrument     TEXT,
    ADD COLUMN amount_refunded_paise  BIGINT      NOT NULL DEFAULT 0,
    -- Set when a payment needs human attention (e.g. a late payment whose
    -- automatic refund failed). Surfaced in the admin console.
    ADD COLUMN payment_attention      TEXT,
    ADD CONSTRAINT orders_rzp_order_id_format CHECK (razorpay_order_id IS NULL OR razorpay_order_id ~ '^order_[A-Za-z0-9]{6,40}$'),
    ADD CONSTRAINT orders_rzp_payment_id_format CHECK (razorpay_payment_id IS NULL OR razorpay_payment_id ~ '^pay_[A-Za-z0-9]{6,40}$'),
    ADD CONSTRAINT orders_refunded_range CHECK (amount_refunded_paise >= 0 AND amount_refunded_paise <= total_paise),
    ADD CONSTRAINT orders_payment_attention_len CHECK (payment_attention IS NULL OR char_length(payment_attention) <= 300),
    ADD CONSTRAINT orders_pending_payment_expiry CHECK (status <> 'pending_payment' OR payment_expires_at IS NOT NULL);

CREATE UNIQUE INDEX orders_razorpay_order_id_key ON orders (razorpay_order_id) WHERE razorpay_order_id IS NOT NULL;
CREATE UNIQUE INDEX orders_razorpay_payment_id_key ON orders (razorpay_payment_id) WHERE razorpay_payment_id IS NOT NULL;
-- The expiry sweep only ever scans unpaid orders.
CREATE INDEX orders_payment_expiry_idx ON orders (payment_expires_at) WHERE status = 'pending_payment';

ALTER TABLE stock_movements DROP CONSTRAINT stock_movements_reason;
ALTER TABLE stock_movements ADD CONSTRAINT stock_movements_reason CHECK (
    reason IN ('initial', 'order_placed', 'order_cancelled', 'payment_expired', 'payment_revived',
               'manual_set', 'manual_adjust', 'restock')
);

-- Every Razorpay webhook delivery, deduplicated by Razorpay's event id
-- (`X-Razorpay-Event-Id`). Redeliveries hit the primary key and are ignored.
CREATE TABLE payment_events (
    id                   TEXT PRIMARY KEY,
    event                TEXT        NOT NULL,
    order_id             UUID REFERENCES orders (id) ON DELETE SET NULL,
    razorpay_order_id    TEXT,
    razorpay_payment_id  TEXT,
    payload              JSONB       NOT NULL,
    received_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at         TIMESTAMPTZ,
    outcome              TEXT,
    CONSTRAINT payment_events_id_len CHECK (char_length(id) BETWEEN 1 AND 100),
    CONSTRAINT payment_events_event_len CHECK (char_length(event) BETWEEN 1 AND 100),
    CONSTRAINT payment_events_outcome_len CHECK (outcome IS NULL OR char_length(outcome) <= 300)
);

CREATE INDEX payment_events_order_idx ON payment_events (order_id, received_at DESC);
CREATE INDEX payment_events_received_idx ON payment_events (received_at);

-- Refunds issued through the Razorpay API (admin-initiated or automatic).
CREATE TABLE refunds (
    id                   UUID PRIMARY KEY,
    order_id             UUID        NOT NULL REFERENCES orders (id) ON DELETE RESTRICT,
    razorpay_payment_id  TEXT        NOT NULL,
    razorpay_refund_id   TEXT,
    amount_paise         BIGINT      NOT NULL,
    status               TEXT        NOT NULL DEFAULT 'pending',
    reason               TEXT        NOT NULL,
    initiated_by         UUID REFERENCES admins (id) ON DELETE SET NULL,
    failure_reason       TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT refunds_amount_positive CHECK (amount_paise > 0),
    CONSTRAINT refunds_status CHECK (status IN ('pending', 'processed', 'failed')),
    CONSTRAINT refunds_reason_len CHECK (char_length(reason) BETWEEN 1 AND 300),
    CONSTRAINT refunds_failure_len CHECK (failure_reason IS NULL OR char_length(failure_reason) <= 300),
    CONSTRAINT refunds_rzp_id_format CHECK (razorpay_refund_id IS NULL OR razorpay_refund_id ~ '^rfnd_[A-Za-z0-9]{6,40}$')
);

CREATE UNIQUE INDEX refunds_razorpay_refund_id_key ON refunds (razorpay_refund_id) WHERE razorpay_refund_id IS NOT NULL;
CREATE INDEX refunds_order_idx ON refunds (order_id, created_at DESC);
