-- Seed the storefront catalog (previously hard-coded in the frontend's
-- src/data/crackers.ts). Prices are in paise; initial stock and bay locations
-- reproduce the frontend's former defaults (stock = 120 - (idx % 6) * 15,
-- bay by idx % 3) and sort_order = position in that list.
--
-- Idempotent: products that already exist are left untouched, and an `initial`
-- stock movement is recorded only for rows inserted here.

WITH seeded AS (
    INSERT INTO products (
        id, name, tamil_name, category, brand, pack_info, pieces_per_pack,
        price_paise, original_price_paise, image_url, gallery,
        description,
        sound_level, is_popular, is_best_selling, is_new_arrival, is_party, stock, bay_location, sort_order
    )
    VALUES
    ('zm-01', '2.75" Bird', NULL, 'SINGLE SOUND CRACKERS', 'Zhi-mart Fireworks', '25 Packet(s)', 25,
     20500, 37300, '/assets/products/lakshmi_box.jpg', '{}'::text[],
     'Classic high-decibel single blast paper wrapped bird crackers. Manufactured with certified eco-nitrate blend in Sivakasi.',
     'Single Loud Blast', true, true, false, false, 120, 'Bay B-1 (Sound Bunker)', 0),
    ('zm-02', '3.5" Lakshmi', NULL, 'SINGLE SOUND CRACKERS', 'Zhi-mart Fireworks', '10 Packet(s)', 10,
     13000, 23600, '/assets/products/lakshmi_box.jpg', '{}'::text[],
     'Authentic 3.5 inch Lakshmi single blast crackers. Clean red paper wrap, crisp sharp detonation, Sivakasi certified.',
     'Single Loud Blast', true, true, false, false, 105, 'Bay A-2 (Aerial Shells)', 1),
    ('zm-03', '4" Lakshmi', NULL, 'SINGLE SOUND CRACKERS', 'Zhi-mart Fireworks', '10 Packet(s)', 10,
     16500, 30000, '/assets/products/lakshmi_box.jpg', '{}'::text[],
     'Four-inch premium Lakshmi cracker box. Extended delay Visco fuse with robust thunderous explosion.',
     'Single Loud Blast', true, false, true, false, 90, 'Bay C-1 (Sparklers Hall)', 2),
    ('zm-04', '4" Deluxe Lakshmi', NULL, 'SINGLE SOUND CRACKERS', 'Zhi-mart Fireworks', '10 Packet(s)', 10,
     27500, 50000, '/assets/products/lakshmi_box.jpg', '{}'::text[],
     'Heavy powder charge deluxe Lakshmi crackers. Maximum reverberation and high-grade unbleached paper sealing.',
     'Single Loud Blast', true, true, false, false, 75, 'Bay B-1 (Sound Bunker)', 3),
    ('zm-05', '28 Giant (TAPE)', NULL, 'ELECTRIC TAPE FUSE CRACKERS', 'Zhi-mart Fireworks', '10 PACKET', 28,
     18500, 33600, '/assets/products/kurukshetra_box.jpg', '{}'::text[],
     'Fast paced continuous tape roll crackers. 28 shots woven on heavy reinforced red adhesive strip.',
     'Repetitive Blast', true, false, false, true, 60, 'Bay A-2 (Aerial Shells)', 4),
    ('zm-06', '56 Giant (TAPE)', NULL, 'ELECTRIC TAPE FUSE CRACKERS', 'Zhi-mart Fireworks', '10 Packet(s)', 56,
     37000, 67200, '/assets/products/kurukshetra_box.jpg', '{}'::text[],
     'Rapid 56-shot thunderous garland strip. Ideal for grand festival inaugurations and evening celebrations.',
     'Repetitive Blast', false, true, false, true, 45, 'Bay C-1 (Sparklers Hall)', 5),
    ('zm-07', '24 Deluxe (2.75" Shell)', NULL, 'DELUXE WIRE FUSE CRACKERS', 'Zhi-mart Fireworks', '1 Packet(s)', 24,
     5000, 9000, '/assets/products/kurukshetra_box.jpg', '{}'::text[],
     'Single packet 24 deluxe wire fuse crackling strip. Tested zero-misfire Visco chain ignition.',
     'Repetitive Blast', false, false, true, false, 120, 'Bay B-1 (Sound Bunker)', 6),
    ('zm-08', '50 Deluxe (2.75" Shell)', NULL, 'DELUXE WIRE FUSE CRACKERS', 'Zhi-mart Fireworks', '1 Packet(s)', 50,
     10500, 19000, '/assets/products/kurukshetra_box.jpg', '{}'::text[],
     'Continuous 50 count wire fuse cracker garland. Clear acoustic pitch and minimal residual smoke.',
     'Repetitive Blast', true, false, false, true, 105, 'Bay A-2 (Aerial Shells)', 7),
    ('zm-09', '100 Deluxe (2.75" Shell)', NULL, 'DELUXE WIRE FUSE CRACKERS', 'Zhi-mart Fireworks', '1 Packet(s)', 100,
     21000, 38200, '/assets/products/kurukshetra_box.jpg', '{}'::text[],
     'Full 100-shot celebration garland. Famous Sivakasi Kurukshetra deluxe edition with unbroken firing sequence.',
     'Repetitive Blast', true, true, false, true, 90, 'Bay C-1 (Sparklers Hall)', 8),
    ('zm-10', 'Ganga Jamuna(Shower+Blast)', NULL, 'FLOWER POTS', 'Zhi-mart Fireworks', '1 Box(es)', 5,
     6500, 11800, '/assets/products/flowerpots_box.jpg', '{}'::text[],
     'Dual phase effect: 5-meter vertical shower of golden fountains terminating in a surprise acoustic blast.',
     'Silent & Visual', true, false, true, true, 75, 'Bay B-1 (Sound Bunker)', 9),
    ('zm-11', 'Flower Pots Big', NULL, 'FLOWER POTS', 'Zhi-mart Fireworks', '1 Box(es)', 10,
     7500, 13600, '/assets/products/flowerpots_box.jpg', '{}'::text[],
     'Traditional wide cone flower pots yielding thick silver showers up to 4 meters high. Long 45-second duration.',
     'Silent & Visual', true, true, false, false, 60, 'Bay A-2 (Aerial Shells)', 10),
    ('zm-12', 'Flower Pots Super Deluxe', NULL, 'FLOWER POTS', 'Zhi-mart Fireworks', '1 Box(es)', 10,
     8300, 15000, '/assets/products/flowerpots_box.jpg', '{}'::text[],
     'Super deluxe grade ceramic flower pots with vibrant green and golden titanium shower plumes.',
     'Silent & Visual', true, true, false, false, 45, 'Bay C-1 (Sparklers Hall)', 11),
    ('zm-13', 'Chakkars Special (Ground Spinner)', NULL, 'GROUND CHAKKARS', 'Zhi-mart Fireworks', '10 Pcs/Box', 10,
     9500, 17300, '/assets/products/chakra_box.jpg', '{}'::text[],
     'High velocity ground spinners producing concentric wheels of dazzling sparks with smooth balanced rotation.',
     'Silent & Visual', false, true, true, false, 120, 'Bay B-1 (Sound Bunker)', 12),
    ('zm-14', '12 Shot Multi Color Aerial Repeater', NULL, 'AERIAL REPEATERS', 'Zhi-mart Fireworks', '1 Box', 12,
     42000, 76400, '/assets/products/aerial_box.jpg', '{}'::text[],
     '12 vertical shots bursting at 30 meters height with willow crowns and crackling champagne strobes.',
     'Repetitive Blast', true, false, true, true, 105, 'Bay A-2 (Aerial Shells)', 13),
    ('zm-15', 'Zhi-mart Mega Family Keepsake Chest', NULL, 'GIFT BOXES & HAMPERS', 'Zhi-mart Fireworks', '38 Items Master Box', 38,
     145000, 263600, '/assets/products/hamper_box.jpg', '{}'::text[],
     'Curated festival gift hamper containing an assortment of sparklers, flower pots, ground chakkars, and aerial shots.',
     'Repetitive Blast', true, true, false, true, 90, 'Bay C-1 (Sparklers Hall)', 14)
    ON CONFLICT (id) DO NOTHING
    RETURNING id, stock
)
INSERT INTO stock_movements (product_id, delta, stock_after, reason, note)
SELECT id, stock, stock, 'initial', 'Opening stock (catalog seed)'
FROM seeded
WHERE stock > 0;
