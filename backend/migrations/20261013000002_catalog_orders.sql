-- Catalog, inventory and orders. The database is the single source of truth:
-- money is stored as integer paise, stock can never go negative, and every
-- stock change is recorded in an append-only ledger.

CREATE TABLE products (
    id                   TEXT PRIMARY KEY,
    name                 TEXT        NOT NULL,
    tamil_name           TEXT,
    category             TEXT        NOT NULL,
    brand                TEXT        NOT NULL,
    pack_info            TEXT        NOT NULL,
    pieces_per_pack      INTEGER,
    price_paise          BIGINT      NOT NULL,
    original_price_paise BIGINT      NOT NULL,
    image_url            TEXT        NOT NULL,
    gallery              TEXT[]      NOT NULL DEFAULT '{}',
    description          TEXT        NOT NULL DEFAULT '',
    sound_level          TEXT,
    is_popular           BOOLEAN     NOT NULL DEFAULT FALSE,
    is_best_selling      BOOLEAN     NOT NULL DEFAULT FALSE,
    is_new_arrival       BOOLEAN     NOT NULL DEFAULT FALSE,
    is_party             BOOLEAN     NOT NULL DEFAULT FALSE,
    stock                INTEGER     NOT NULL DEFAULT 0,
    low_stock_threshold  INTEGER     NOT NULL DEFAULT 20,
    bay_location         TEXT,
    -- Archived products are hidden from the storefront but kept for order history.
    is_active            BOOLEAN     NOT NULL DEFAULT TRUE,
    sort_order           INTEGER     NOT NULL DEFAULT 0,
    -- Optimistic-concurrency token: bumped on every write.
    version              INTEGER     NOT NULL DEFAULT 1,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT products_id_format CHECK (id ~ '^[a-z0-9][a-z0-9-]{1,39}$'),
    CONSTRAINT products_name_len CHECK (char_length(name) BETWEEN 1 AND 120),
    CONSTRAINT products_tamil_name_len CHECK (tamil_name IS NULL OR char_length(tamil_name) <= 120),
    CONSTRAINT products_category_len CHECK (char_length(category) BETWEEN 1 AND 80),
    CONSTRAINT products_brand_len CHECK (char_length(brand) BETWEEN 1 AND 80),
    CONSTRAINT products_pack_info_len CHECK (char_length(pack_info) BETWEEN 1 AND 60),
    CONSTRAINT products_pieces_positive CHECK (pieces_per_pack IS NULL OR pieces_per_pack > 0),
    CONSTRAINT products_price_positive CHECK (price_paise > 0 AND price_paise <= 100000000),
    CONSTRAINT products_mrp_gte_price CHECK (original_price_paise >= price_paise AND original_price_paise <= 100000000),
    CONSTRAINT products_image_len CHECK (char_length(image_url) BETWEEN 1 AND 2048),
    CONSTRAINT products_gallery_len CHECK (cardinality(gallery) <= 10),
    CONSTRAINT products_description_len CHECK (char_length(description) <= 4000),
    CONSTRAINT products_sound_level CHECK (
        sound_level IS NULL OR sound_level IN ('Low Sound', 'Single Loud Blast', 'Repetitive Blast', 'Silent & Visual')
    ),
    CONSTRAINT products_stock_nonneg CHECK (stock >= 0 AND stock <= 10000000),
    CONSTRAINT products_low_stock_nonneg CHECK (low_stock_threshold >= 0),
    CONSTRAINT products_bay_len CHECK (bay_location IS NULL OR char_length(bay_location) <= 80)
);

CREATE INDEX products_storefront_idx ON products (sort_order, id) WHERE is_active;
CREATE INDEX products_updated_idx ON products (updated_at);

-- Human-readable order numbers: ZM-<yymmdd>-<seq>.
CREATE SEQUENCE order_number_seq START WITH 1001;

CREATE TABLE orders (
    id                 UUID PRIMARY KEY,
    order_number       TEXT        NOT NULL,
    -- Client-generated per checkout attempt; a retried submit returns the
    -- original order instead of creating a duplicate (and double-reserving stock).
    idempotency_key    UUID        NOT NULL,
    customer_name      TEXT        NOT NULL,
    customer_phone     TEXT        NOT NULL,
    customer_email     TEXT,
    address_line       TEXT        NOT NULL,
    city               TEXT        NOT NULL,
    state              TEXT        NOT NULL,
    pincode            TEXT        NOT NULL,
    notes              TEXT,
    item_count         INTEGER     NOT NULL,
    mrp_total_paise    BIGINT      NOT NULL,
    total_paise        BIGINT      NOT NULL,
    status             TEXT        NOT NULL DEFAULT 'pending_pack',
    payment_method     TEXT        NOT NULL DEFAULT 'pay_later',
    payment_status     TEXT        NOT NULL DEFAULT 'pending',
    payment_reference  TEXT,
    paid_at            TIMESTAMPTZ,
    refunded_at        TIMESTAMPTZ,
    transport_partner  TEXT,
    docket_number      TEXT,
    dispatched_at      TIMESTAMPTZ,
    delivered_at       TIMESTAMPTZ,
    cancelled_at       TIMESTAMPTZ,
    cancel_reason      TEXT,
    client_ip          INET,
    user_agent         TEXT,
    version            INTEGER     NOT NULL DEFAULT 1,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT orders_status CHECK (
        status IN ('pending_pack', 'dispatched', 'in_transit', 'out_for_delivery', 'delivered', 'cancelled')
    ),
    CONSTRAINT orders_payment_method CHECK (payment_method IN ('pay_later', 'upi', 'card', 'netbanking', 'cod', 'bank_transfer')),
    CONSTRAINT orders_payment_status CHECK (payment_status IN ('pending', 'paid', 'refunded', 'failed')),
    CONSTRAINT orders_name_len CHECK (char_length(customer_name) BETWEEN 2 AND 100),
    CONSTRAINT orders_phone_format CHECK (customer_phone ~ '^\+91[6-9][0-9]{9}$'),
    CONSTRAINT orders_email_len CHECK (customer_email IS NULL OR char_length(customer_email) <= 254),
    CONSTRAINT orders_address_len CHECK (char_length(address_line) BETWEEN 5 AND 300),
    CONSTRAINT orders_city_len CHECK (char_length(city) BETWEEN 2 AND 80),
    CONSTRAINT orders_state_len CHECK (char_length(state) BETWEEN 2 AND 80),
    CONSTRAINT orders_pincode_format CHECK (pincode ~ '^[1-9][0-9]{5}$'),
    CONSTRAINT orders_notes_len CHECK (notes IS NULL OR char_length(notes) <= 500),
    CONSTRAINT orders_totals_nonneg CHECK (total_paise > 0 AND mrp_total_paise >= total_paise),
    CONSTRAINT orders_item_count_positive CHECK (item_count > 0),
    CONSTRAINT orders_payment_ref_len CHECK (payment_reference IS NULL OR char_length(payment_reference) <= 120),
    CONSTRAINT orders_transport_len CHECK (transport_partner IS NULL OR char_length(transport_partner) <= 80),
    CONSTRAINT orders_docket_len CHECK (docket_number IS NULL OR char_length(docket_number) <= 80),
    CONSTRAINT orders_cancel_reason_len CHECK (cancel_reason IS NULL OR char_length(cancel_reason) <= 300)
);

CREATE UNIQUE INDEX orders_order_number_key ON orders (order_number);
CREATE UNIQUE INDEX orders_idempotency_key ON orders (idempotency_key);
CREATE INDEX orders_created_idx ON orders (created_at DESC);
CREATE INDEX orders_status_created_idx ON orders (status, created_at DESC);
CREATE INDEX orders_payment_status_idx ON orders (payment_status, created_at DESC);
CREATE INDEX orders_updated_idx ON orders (updated_at);
CREATE INDEX orders_phone_idx ON orders (customer_phone);
-- Per-IP order-placement throttling looks back over recent orders.
CREATE INDEX orders_client_ip_created_idx ON orders (client_ip, created_at DESC);

-- Line items snapshot name/price at purchase time; later catalog edits never
-- rewrite order history.
CREATE TABLE order_items (
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    order_id          UUID    NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    product_id        TEXT    NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    product_name      TEXT    NOT NULL,
    pack_info         TEXT    NOT NULL,
    unit_price_paise  BIGINT  NOT NULL,
    unit_mrp_paise    BIGINT  NOT NULL,
    quantity          INTEGER NOT NULL,
    line_total_paise  BIGINT  NOT NULL,
    CONSTRAINT order_items_quantity CHECK (quantity BETWEEN 1 AND 1000),
    CONSTRAINT order_items_prices CHECK (unit_price_paise > 0 AND unit_mrp_paise >= unit_price_paise),
    CONSTRAINT order_items_line_total CHECK (line_total_paise = unit_price_paise * quantity),
    CONSTRAINT order_items_unique_product UNIQUE (order_id, product_id)
);

CREATE INDEX order_items_product_idx ON order_items (product_id);

-- Append-only inventory ledger. stock_after makes the history auditable
-- without replaying every row.
CREATE TABLE stock_movements (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id   TEXT        NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    delta        INTEGER     NOT NULL,
    stock_after  INTEGER     NOT NULL,
    reason       TEXT        NOT NULL,
    order_id     UUID REFERENCES orders (id) ON DELETE SET NULL,
    admin_id     UUID REFERENCES admins (id) ON DELETE SET NULL,
    note         TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT stock_movements_delta_nonzero CHECK (delta <> 0),
    CONSTRAINT stock_movements_after_nonneg CHECK (stock_after >= 0),
    CONSTRAINT stock_movements_reason CHECK (
        reason IN ('initial', 'order_placed', 'order_cancelled', 'manual_set', 'manual_adjust', 'restock')
    ),
    CONSTRAINT stock_movements_note_len CHECK (note IS NULL OR char_length(note) <= 300)
);

CREATE INDEX stock_movements_product_created_idx ON stock_movements (product_id, created_at DESC);
CREATE INDEX stock_movements_order_idx ON stock_movements (order_id) WHERE order_id IS NOT NULL;
