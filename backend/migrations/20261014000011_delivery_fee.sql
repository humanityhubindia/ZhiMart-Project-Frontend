-- Delivery charge. An order whose goods ("subtotal", after tier pricing) come to
-- less than FREE_DELIVERY_MIN_RUPEES (default ₹5,000, inclusive) pays a flat
-- DELIVERY_FEE_RUPEES (default ₹350); otherwise delivery is free. The fee is
-- recorded per order and included in `total_paise`, which stays the payable
-- amount (Razorpay amount, refunds, revenue):
--     total_paise = sum(order_items.line_total_paise) + delivery_fee_paise
-- Every order placed before this migration had free delivery (fee 0).
-- (Also from this release: STANDARD_DISCOUNT_PERCENT defaults to 75 %, not the
-- 70 % that 20261013000010_pricing_tier.sql mentions. Config only; no schema change.)
ALTER TABLE orders
    ADD COLUMN delivery_fee_paise BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT orders_delivery_fee_nonneg CHECK (delivery_fee_paise >= 0);

-- A small order plus its delivery fee can cost more than the goods' MRP, so the
-- MRP comparison is now against the goods only (total - fee), which must still
-- be positive.
ALTER TABLE orders DROP CONSTRAINT orders_totals_nonneg;
ALTER TABLE orders ADD CONSTRAINT orders_totals_nonneg CHECK (
    total_paise - delivery_fee_paise > 0
    AND mrp_total_paise >= total_paise - delivery_fee_paise
);
