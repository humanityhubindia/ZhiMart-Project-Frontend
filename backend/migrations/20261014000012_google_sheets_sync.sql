-- Google Sheets order sync: one spreadsheet row per order, appended when the
-- order is placed and rewritten in place whenever the order changes.
--
-- `sheet_sync_queue` holds one row per order whose spreadsheet row is out of
-- date. Triggers on `orders` add (or re-dirty) the order in the SAME
-- transaction as the change, so every code path (checkout, admin edits,
-- Razorpay verify/webhook, refunds, the expiry sweep) is covered and nothing is
-- queued for a rolled-back change. A background worker (`jobs.rs`) claims due
-- rows with SKIP LOCKED + a lease (safe across replicas), writes the current
-- order data to the sheet, and deletes the row only if `revision` is unchanged
-- (compare-and-clear): a change that lands mid-sync bumps `revision`, so the
-- row stays queued and is synced again. The worker's bookkeeping touches this
-- table only, never `orders`, so it cannot re-dirty an order.
--
-- No backfill: orders that exist when this migration runs get
-- `sync_to_sheet = false` and are never queued, even when edited later. Every
-- order inserted afterwards defaults to true.
--
-- Rows are queued whether or not GOOGLE_SHEETS_ENABLED is on (the database
-- cannot see the env). With the feature off nothing consumes the queue, so
-- turning it on later syncs every order placed since this migration. To skip
-- those, run `DELETE FROM sheet_sync_queue;` before enabling.

ALTER TABLE orders ADD COLUMN sync_to_sheet BOOLEAN NOT NULL DEFAULT FALSE;
-- Existing rows keep FALSE (the value at ADD COLUMN time); new orders get TRUE.
ALTER TABLE orders ALTER COLUMN sync_to_sheet SET DEFAULT TRUE;

CREATE TABLE sheet_sync_queue (
    order_id         UUID PRIMARY KEY REFERENCES orders (id) ON DELETE CASCADE,
    -- Bumped by every change to the order; the worker clears the row only if
    -- the revision it synced is still current.
    revision         BIGINT      NOT NULL DEFAULT 1,
    -- Last time the order changed (for ops: how stale is the sheet?).
    requested_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Failed/claimed attempts since the last success (drives the backoff).
    attempts         INTEGER     NOT NULL DEFAULT 0,
    -- Due time; a claim pushes it out by the lease, a failure by the backoff.
    next_attempt_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT sheet_sync_queue_attempts_nonneg CHECK (attempts >= 0),
    CONSTRAINT sheet_sync_queue_error_len CHECK (last_error IS NULL OR char_length(last_error) <= 500)
);

CREATE INDEX sheet_sync_queue_due_idx ON sheet_sync_queue (next_attempt_at);

-- Queue (or re-dirty) the order. A re-dirty keeps `next_attempt_at`: if the row
-- is leased by a worker right now, it must not become claimable by another one
-- (that could append the order twice); the worker notices the new revision and
-- releases the row for an immediate re-sync. A row in failure backoff stays in
-- backoff (Google is the problem, not the order).
CREATE FUNCTION orders_queue_sheet_sync() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO sheet_sync_queue (order_id) VALUES (NEW.id)
    ON CONFLICT (order_id) DO UPDATE
        SET revision = sheet_sync_queue.revision + 1,
            requested_at = now();
    RETURN NULL;
END;
$$;

CREATE TRIGGER orders_sheet_sync_insert
    AFTER INSERT ON orders
    FOR EACH ROW WHEN (NEW.sync_to_sheet)
    EXECUTE FUNCTION orders_queue_sheet_sync();

-- Any real change to the row (status, payment, refund totals, shipping, …).
-- Payment state lives on `orders` itself (payment_status, paid_at,
-- amount_refunded_paise, …; `refunds` rows are summarised onto it), so this one
-- trigger covers payments and refunds too.
CREATE TRIGGER orders_sheet_sync_update
    AFTER UPDATE ON orders
    FOR EACH ROW WHEN (NEW.sync_to_sheet AND OLD.* IS DISTINCT FROM NEW.*)
    EXECUTE FUNCTION orders_queue_sheet_sync();
