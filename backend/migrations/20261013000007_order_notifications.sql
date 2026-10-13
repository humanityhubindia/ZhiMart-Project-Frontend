-- Transactional outbox for emails. Rows are inserted in the SAME transaction as
-- the business change (order placed / payment confirmed), so a notification is
-- never lost and never sent for a rolled-back order. A background worker claims
-- due rows with SKIP LOCKED (safe across replicas), renders the email from the
-- current order data, sends it, and retries with exponential backoff.

CREATE TABLE email_outbox (
    id               UUID PRIMARY KEY,
    kind             TEXT        NOT NULL,
    order_id         UUID REFERENCES orders (id) ON DELETE CASCADE,
    status           TEXT        NOT NULL DEFAULT 'pending',
    attempts         INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error       TEXT,
    sent_at          TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT email_outbox_kind CHECK (kind IN ('admin_order_notification')),
    CONSTRAINT email_outbox_status CHECK (status IN ('pending', 'sent', 'failed')),
    CONSTRAINT email_outbox_attempts_nonneg CHECK (attempts >= 0),
    CONSTRAINT email_outbox_error_len CHECK (last_error IS NULL OR char_length(last_error) <= 500)
);

-- At most one notification of each kind per order (enqueue is idempotent).
CREATE UNIQUE INDEX email_outbox_kind_order_key ON email_outbox (kind, order_id);
CREATE INDEX email_outbox_due_idx ON email_outbox (next_attempt_at) WHERE status = 'pending';
CREATE INDEX email_outbox_created_idx ON email_outbox (created_at);
