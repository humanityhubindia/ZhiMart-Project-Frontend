-- Two-tier pricing. An order whose value at price-list prices (`products.price_paise`)
-- reaches BULK_MIN_ORDER_RUPEES (default ₹10,000) is charged those prices ("bulk");
-- a smaller order pays the "standard" price on every line (MRP less
-- STANDARD_DISCOUNT_PERCENT, default 70 %, rounded up to a whole rupee, never below
-- the list price). The tier an order was charged at is recorded here; every order
-- placed before this migration was charged list prices, i.e. bulk.
ALTER TABLE orders
    ADD COLUMN pricing_tier TEXT NOT NULL DEFAULT 'bulk',
    ADD CONSTRAINT orders_pricing_tier CHECK (pricing_tier IN ('bulk', 'standard'));
