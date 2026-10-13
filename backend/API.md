# Admin Auth API (v1)

Base path: `/api/v1`. All bodies are JSON (`camelCase`). Every auth response carries
`Cache-Control: no-store`.

## Flow

```
email + password ──► POST /auth/login ──► 6-digit code emailed (AWS SES)
                                   │
            code ──► POST /auth/otp/verify ──► access token (JSON, 15 min)
                                              + refresh cookie (HttpOnly, 30-day hard cap)
                                   │
   access token expires ──► POST /auth/refresh ──► new access token + rotated refresh cookie
   day 30 reached       ──► 401 session_expired ──► must log in again (password + OTP)
```

- **Access token**: HS256 JWT, 15 min (`ACCESS_TOKEN_TTL_SECS`), sent as
  `Authorization: Bearer <token>`. The frontend keeps it **in memory only**.
  Claims: `sub` (admin id), `sid` (session id), `iss`, `aud`, `iat`, `nbf`, `exp`, `jti`.
  `exp` never exceeds the session's absolute expiry. Every authenticated request
  also checks the session row, so logout/revocation takes effect immediately.
- **Refresh token**: 256-bit random, opaque, stored server-side as SHA-256 only.
  Cookie `__Secure-zm_rt` (or `zm_rt` when `COOKIE_SECURE=false` for local http):
  `HttpOnly; Secure; SameSite=Strict; Path=/api/v1/auth; Max-Age=<seconds left in session>`.
  Rotated on every refresh. Re-use of an already-rotated token revokes the whole session,
  with one exception: a **retry grace window** — if the previous token is presented again
  within 30 s of its rotation *and* the token it was rotated into has never been used
  (the refresh response was lost), the retry succeeds and returns a fresh token. The frontend
  should still run at most one refresh at a time (single-flight).
- **Trusted-device cookie**: set on successful `/auth/otp/verify`. Cookie `__Secure-zm_dev`
  (or `zm_dev` when `COOKIE_SECURE=false`): `HttpOnly; Secure; SameSite=Strict;
  Path=/api/v1/auth; Max-Age=15552000` (180 days), random value, stored as SHA-256 bound to
  the admin. It is renewed on each full login from that browser and revoked by a password
  reset. The browser sends it automatically to `/auth/login` and `/auth/otp/*`
  (`credentials: "include"`); the frontend never reads it.
- **Session**: created at OTP verification with `absolute_expires_at = now + 30 days`.
  Rotation never extends it.
- **CSRF**: cookie-authenticated endpoints (`/auth/refresh`, `/auth/logout`) require header
  `X-Requested-With: XMLHttpRequest`, and if an `Origin` header is present it must be in
  `ALLOWED_ORIGINS`.
- **Brute-force protection** (password *and* OTP attempts):
  - *Per (email, client IP)* — after `LOGIN_MAX_FAILURES` (default 5) failures that email/IP
    pair is throttled for `LOGIN_LOCKOUT_SECS` (default 15 min), doubling on each repeat up
    to 24 h. Behaves identically for registered and unregistered emails and answers
    `429 too_many_requests` with `retryAfter`. Other IPs (e.g. the real admin) are unaffected.
    Attempts are counted *before* they are evaluated, so parallel requests cannot exceed
    the budget.
  - *Account-wide ceiling* — 50 failures within an hour across all IPs locks the account for
    15 min (`429 account_locked`) **for browsers without a trusted-device cookie**. A browser
    that has completed a login before keeps working.

## Error shape

```json
{ "error": { "code": "otp_invalid", "message": "The verification code is incorrect.", "attemptsRemaining": 3 } }
```

| HTTP | code                    | meaning                                                         |
|------|-------------------------|-----------------------------------------------------------------|
| 400  | `validation_error`      | malformed input                                                 |
| 401  | `invalid_credentials`   | wrong email or password (never says which)                      |
| 401  | `otp_invalid`           | wrong code; `attemptsRemaining` included                        |
| 401  | `otp_challenge_expired` | challenge expired / used / out of attempts → restart login      |
| 401  | `unauthorized`          | missing/invalid/expired access token → try `/auth/refresh`      |
| 401  | `session_expired`       | refresh failed or 30-day limit reached → show login             |
| 403  | `forbidden`             | CSRF/origin check failed                                        |
| 429  | `too_many_requests`     | rate limited / too many failures from this email+IP / OTP issuance cap / resend cooldown / brief lock contention; `retryAfter` seconds + `Retry-After` header |
| 429  | `account_locked`        | account-wide failure ceiling reached (distributed attack); only for browsers without a trusted-device cookie; `retryAfter` seconds. Suggested UI: "Sign-in from new devices is temporarily paused for this account." |
| 503  | `service_unavailable`   | DB pool exhausted or email provider down                        |

## Endpoints

### `POST /auth/login`
Request `{ "email": "admin@zhimart.com", "password": "..." }`
(email ≤ 254 chars, trimmed + lowercased server-side; password 1–256 chars)

200
```json
{ "challengeId": "0199…uuid", "maskedEmail": "a***n@zhimart.com", "expiresIn": 300, "resendAvailableIn": 60 }
```
Errors: `invalid_credentials`, `too_many_requests` (this email+IP is throttled, or 5 codes were
already issued in 15 min), `account_locked` (account-wide ceiling, untrusted browser),
`validation_error`, `service_unavailable`.

### `POST /auth/otp/verify`
Request `{ "challengeId": "uuid", "code": "123456" }` (exactly 6 ASCII digits)

200 + `Set-Cookie` refresh cookie + `Set-Cookie` trusted-device cookie. This is the same
`AuthSession` shape that `/auth/refresh` returns.
```json
{
  "accessToken": "eyJ…",
  "tokenType": "Bearer",
  "expiresIn": 900,
  "sessionExpiresAt": "2026-11-12T10:00:00Z",
  "admin": { "id": "uuid", "email": "admin@zhimart.com", "displayName": "Admin (Operations)" }
}
```
Errors: `otp_invalid` (with `attemptsRemaining`), `otp_challenge_expired`, `account_locked`,
`too_many_requests` (this email+IP is throttled — wrong codes count toward the same budget as
wrong passwords — or the challenge is briefly busy; a throttled attempt does not use up an
OTP attempt).

### `POST /auth/otp/resend`
Request `{ "challengeId": "uuid" }` → 200 `{ "expiresIn": 300, "resendAvailableIn": 60 }`
Issues a fresh code, resets attempts and the expiry window; the old code stops working.
Errors: `too_many_requests` (cooldown, with `retryAfter`), `otp_challenge_expired` (unknown/used/expired
or max resends reached), `account_locked`, `service_unavailable`.

### `POST /auth/refresh`
Cookie + `X-Requested-With: XMLHttpRequest`, empty body → 200 `AuthSession` (above) + rotated cookie.
Errors: `session_expired` (cookie is cleared), `forbidden`, `too_many_requests` (a concurrent
refresh holds the session; retry), `service_unavailable` (cookie kept; retry).

### `POST /auth/logout`
Cookie + `X-Requested-With: XMLHttpRequest` (Bearer optional) → 204, cookie cleared, session revoked.
Idempotent: returns 204 whether or not a session was found. Only a database failure yields
500/503 (the cookie is still cleared).

### `GET /auth/me`
Bearer → 200 `{ "admin": { … }, "sessionExpiresAt": "…" }`. Errors: `unauthorized`.

### Health (no `/api/v1` prefix)
`GET /health/live` → 200 `{"status":"ok"}`. `GET /health/ready` → 200, or 503 if the DB is unreachable.

---

# Catalog, Inventory & Orders API (v1)

The database is the single source of truth. Money is stored as integer **paise**; JSON
speaks **rupees as numbers** (`165`, `165.5`). Timestamps are RFC 3339 UTC. All admin
endpoints require `Authorization: Bearer <accessToken>` (see `AuthAdmin`). Business-day
boundaries ("today", daily charts) use **Asia/Kolkata**.

Additional error codes:

| HTTP | code                  | meaning                                                                 |
|------|-----------------------|-------------------------------------------------------------------------|
| 400  | `validation_error`    | `message` names the offending field when possible                       |
| 404  | `not_found`           | unknown / archived product or order                                     |
| 409  | `out_of_stock`        | `details: [{productId, requested, available}]`                          |
| 409  | `version_conflict`    | someone else changed this record; refetch and retry                     |
| 409  | `invalid_transition`  | illegal order/payment status change                                     |
| 409  | `product_unavailable` | product archived or unknown at checkout                                 |
| 409  | `duplicate_id`        | product id already exists                                               |
| 413  | `payload_too_large`   | body over `MAX_BODY_BYTES` (default 64 KiB)                             |
| 405  | `method_not_allowed`  |                                                                         |

## Shared shapes

`Product` (public storefront):
```json
{
  "id": "zm-03", "name": "4\" Lakshmi", "tamilName": "லட்சுமி", "category": "SINGLE SOUND CRACKERS",
  "brand": "Zhi-mart Fireworks", "packInfo": "10 Packet(s)", "piecesPerPack": 10,
  "price": 165, "originalPrice": 300, "discountPercent": 45,
  "standardPrice": 165, "standardDiscountPercent": 45,
  "image": "/assets/products/lakshmi_box.jpg", "gallery": [], "description": "…",
  "soundLevel": "Single Loud Blast",
  "isPopular": true, "isBestSelling": false, "isNewArrival": false, "isParty": false,
  "inStock": true, "availableQuantity": 120
}
```
`availableQuantity = min(stock, 1000)`; `inStock = stock > 0`.
`price` is the price-list ("bulk") price; `standardPrice` is what the product costs in an
order below the bulk minimum (see **Pricing tiers** below; for zm26-001: `price` 32,
`originalPrice` 210, `standardPrice` 63, `standardDiscountPercent` 70).
`standardDiscountPercent` = discount of `standardPrice` off `originalPrice`. Both are also in
`AdminProduct` (always real prices).

### Pricing tiers
Decided per order, server-side, from the DB prices:
- `listTotal = Σ price × quantity` (real list prices, even in payment test mode).
- `listTotal >= pricing.bulkMinOrder` (default ₹10,000, inclusive) ⇒ **`bulk`**: every line at `price`.
- otherwise ⇒ **`standard`**: every line at `standardPrice =
  max(price, min(originalPrice, ceilToRupee(originalPrice × (100 − standardDiscountPercent) / 100)))`
  (default 70 % off MRP). A product discounted less than that costs `price` in both tiers.
- `PAYMENT_TEST_MODE`: tier recorded as above, but every unit is charged ₹1 (`price` and
  `standardPrice` are both 1 on the storefront).

The rule itself is published as `pricing` (rupees / whole percents; config
`BULK_MIN_ORDER_RUPEES`, `BULK_DISCOUNT_PERCENT` (label only), `STANDARD_DISCOUNT_PERCENT`):
```json
"pricing": { "bulkMinOrder": 10000, "bulkDiscountPercent": 85, "standardDiscountPercent": 70 }
```

`AdminProduct` = `Product` + `{ "stock": 120, "lowStockThreshold": 20, "bayLocation": "Bay B-1", "isActive": true, "sortOrder": 0, "version": 3, "createdAt": "…", "updatedAt": "…" }` (exact `stock`, not capped).

`ProductInput` (admin create/update body):
```json
{
  "id": "zm-21",              // create only, optional: ^[a-z0-9][a-z0-9-]{1,39}$; generated from name if omitted
  "name": "…", "tamilName": null, "category": "…", "brand": "…", "packInfo": "…", "piecesPerPack": 10,
  "price": 165, "originalPrice": 300, "image": "/assets/products/x.jpg", "gallery": [],
  "description": "…", "soundLevel": null,
  "isPopular": false, "isBestSelling": false, "isNewArrival": false, "isParty": false,
  "lowStockThreshold": 20, "bayLocation": null, "sortOrder": 0,
  "initialStock": 100,        // create only (records an `initial` stock movement); stock is edited via /inventory
  "expectedVersion": 3        // update only (required)
}
```
`image`/`gallery` entries: a site-relative path starting with `/assets/` or an `https://` URL; ≤ 2048 chars.
`originalPrice >= price > 0`, ≤ ₹10,00,000, at most 2 decimals.

`Order` (admin):
```json
{
  "id": "uuid", "orderNumber": "ZM-261013-1001", "createdAt": "…", "updatedAt": "…", "version": 2,
  "customer": { "name": "…", "phone": "+919840012345", "email": null,
                "address": "…", "city": "Chennai", "state": "Tamil Nadu", "pincode": "600028" },
  "notes": null,
  "items": [ { "productId": "zm-03", "productName": "4\" Lakshmi", "packInfo": "10 Packet(s)",
               "quantity": 10, "unitPrice": 165, "unitMrp": 300, "lineTotal": 1650 } ],
  "itemCount": 10, "mrpTotal": 3000, "totalAmount": 1650,
  "pricingTier": "standard",
  "status": "pending_pack",
  "payment": { "method": "pay_later", "status": "pending", "reference": null, "paidAt": null, "refundedAt": null },
  "shipping": { "transportPartner": null, "docketNumber": null, "dispatchedAt": null, "deliveredAt": null },
  "cancelledAt": null, "cancelReason": null
}
```
Order `status`: `pending_pack → dispatched → in_transit → out_for_delivery → delivered`, plus
`cancelled`. Forward moves only (skipping ahead allowed); `dispatched` (or later) requires
`transportPartner` and `docketNumber` to be set. Payment `status`: `pending | paid | refunded | failed`;
`method`: `pay_later | upi | card | netbanking | cod | bank_transfer`.
`pricingTier`: `bulk | standard` — the tier the order was charged at (orders placed before
tiered pricing are `bulk`). `items[].unitPrice` is what was actually charged.

## Public endpoints

### `GET /products` → `{ "products": Product[], "testMode": false, "pricing": { "bulkMinOrder": 10000, "bulkDiscountPercent": 85, "standardDiscountPercent": 70 } }`
Active products only, ordered by `sortOrder, id`. `Cache-Control: public, max-age=10`.

### `GET /products/{id}` → `Product` (404 if unknown/archived)

### `POST /orders`  (per-IP `orders` limiter + per-IP hourly cap in DB)
```json
{
  "idempotencyKey": "uuid-v4 generated once per checkout attempt",
  "customer": { "name": "…", "phone": "98400 12345", "email": null,
                "address": "…", "city": "…", "state": "…", "pincode": "600028" },
  "notes": null,
  "items": [ { "productId": "zm-03", "quantity": 10 } ]
}
```
- 1–50 lines, unique `productId`s, quantity 1–1000. Phone accepts `9840012345`, `+91 98400 12345`,
  `09840012345` → stored as `+919840012345`. Prices are **always** taken from the DB, never the client,
  and the tier (`bulk` / `standard`, see **Pricing tiers**) is decided from them.
- Atomic: all lines' stock is decremented in one transaction (rows locked in `productId` order to
  avoid deadlocks) with an `order_placed` stock movement per line, or nothing happens.
- 201 `{ "order": { "orderNumber", "status", "pricingTier", "totalAmount", "mrpTotal", "itemCount", "createdAt", "items": [...] } }`.
  Re-sending the same `idempotencyKey` returns 200 with the original order (no second reservation);
  same key with a different payload → 409 `idempotency_conflict`.
- Errors: 409 `out_of_stock` (with `details`), 409 `product_unavailable`, 400 `validation_error`, 429.

## Admin endpoints (`/admin/...`)

Products
- `GET /admin/products?includeArchived=true|false` → `{ "products": AdminProduct[] }`
- `POST /admin/products` (`ProductInput`) → 201 `AdminProduct`; 409 `duplicate_id`
- `PUT /admin/products/{id}` (`ProductInput` with `expectedVersion`) → `AdminProduct`; 409 `version_conflict`
- `DELETE /admin/products/{id}` → 204 (archive: `isActive=false`, hidden from storefront, history kept)
- `POST /admin/products/{id}/restore` → `AdminProduct`

Inventory
- `GET /admin/inventory` → `{ "items": [{ "productId","name","category","image","stock","lowStockThreshold","bayLocation","isActive","status":"in_stock|low_stock|out_of_stock","version","updatedAt" }], "summary": { "totalUnits","inStock","lowStock","outOfStock" } }`
- `POST /admin/inventory/{productId}/adjust`
  `{ "mode": "set" | "add" | "remove", "quantity": 50, "note": "Batch #12 from factory", "expectedStock": 120 }`
  `set` requires `expectedStock` (current value the admin saw) → 409 `version_conflict` if it moved
  (e.g. an order landed meanwhile). `add` = restock, `remove` = write-off (409 `out_of_stock` if it
  would go negative). Records a `restock` / `manual_adjust` / `manual_set` movement with the admin id.
  → `AdminProduct`
- `GET /admin/inventory/{productId}/movements?limit=50` → `{ "movements": [{ "id","delta","stockAfter","reason","orderNumber","adminEmail","note","createdAt" }] }`

Orders
- `GET /admin/orders?status=&paymentStatus=&q=&limit=50&cursor=` → `{ "orders": Order[], "nextCursor": string|null }`
  Newest first, keyset pagination (opaque cursor). `q` matches order number, customer name, phone, city.
- `GET /admin/orders/{id}` → `Order`
- `POST /admin/orders/{id}/status` `{ "status": "dispatched", "transportPartner": "VRL", "docketNumber": "LR-1", "expectedVersion": 2 }` → `Order`
- `PUT /admin/orders/{id}/shipping` `{ "transportPartner", "docketNumber", "expectedVersion" }` → `Order` (not after `delivered`/`cancelled`)
- `POST /admin/orders/{id}/cancel` `{ "reason": "Customer request", "expectedVersion": 2 }` → `Order`.
  Only before dispatch (`pending_pack`). Restocks every line (`order_cancelled` movements) atomically.
- `POST /admin/orders/{id}/payment` `{ "action": "mark_paid" | "mark_failed" | "mark_refunded", "method": "upi", "reference": "UTR123", "expectedVersion": 2 }` → `Order`.
  `pending|failed → paid` (method required), `pending → failed`, `paid → refunded`.

Dashboard
- `GET /admin/dashboard` →
```json
{
  "revenue": { "today": 0, "last7Days": 0, "last30Days": 0, "allTime": 0, "paid": 0, "pendingCollection": 0 },
  "orders": { "today": 0, "total": 0, "byStatus": { "pending_pack": 0, "dispatched": 0, "in_transit": 0, "out_for_delivery": 0, "delivered": 0, "cancelled": 0 } },
  "inventory": { "activeProducts": 0, "totalUnits": 0, "lowStockCount": 0, "outOfStockCount": 0,
                 "lowStock": [{ "productId","name","stock","lowStockThreshold" }] },
  "salesByDay": [{ "date": "2026-10-13", "orders": 0, "revenue": 0 }],
  "topProducts": [{ "productId","name","unitsSold","revenue" }],
  "recentOrders": [Order]
}
```
  Revenue excludes cancelled orders and refunded payments. `salesByDay` = last 14 IST days (zero-filled),
  `topProducts` = top 5 by units over 30 days, `recentOrders` = latest 5, `lowStock` = up to 10, lowest first.
- `GET /admin/sync` → `{ "ordersVersion": "…", "productsVersion": "…", "pendingOrders": 3, "latestOrderNumber": "ZM-…" }`
  Cheap change marker (max `updated_at` + counts). The admin UI polls this every ~5 s and refetches
  lists only when a version changes.

---

# Payments (Razorpay) & Guest Orders (v1)

Customers never log in. Checkout collects only: name, phone, optional email, address, city,
state, pincode, optional notes. Payment is online via Razorpay Checkout.

**Trust model:** the browser never decides that an order is paid. An order becomes paid only
after the server (a) verifies the Checkout signature AND confirms the payment with the
Razorpay API (amount, currency, order id, status), or (b) receives a signature-verified
webhook. Both paths run the same idempotent service.

## Order lifecycle
```
POST /orders ─► pending_payment (stock RESERVED, paymentExpiresAt = now + PAYMENT_EXPIRY_MINUTES)
      │  POST /orders/{n}/pay ─► Razorpay order created (once) ─► Checkout modal
      │  verified payment (verify endpoint or webhook)
      ▼
 pending_pack ─► dispatched ─► in_transit ─► out_for_delivery ─► delivered
 pending_payment past expiry ─► cancelled (stock released, cancel_reason "Payment not completed")
 payment arrives after expiry ─► stock re-reserved if available ("revived"), else auto-refund
```

## Test mode
`PAYMENT_TEST_MODE=true` ⇒ every product is **charged ₹1 per unit**, enforced server-side:
public catalog `price` = 1 (real `originalPrice` kept), `GET /products` returns
`{ "products": [...], "testMode": true }`, orders are created with `isTest: true` and totals at
₹1 × quantity. `false` ⇒ real prices (production). With `APP_ENV=production` and test mode off,
live keys (`rzp_live_…`) are required. Test orders are excluded from real revenue figures.

## Guest order token
`POST /orders` returns `orderToken` (43-char base64url, HMAC-derived, bound to the order).
It authorizes pay/verify/status for that order only. Send it as header **`X-Order-Token`**
(never in a query string). The storefront keeps it in the URL **fragment**
(`?page=order&order=ZM-…#t=<token>`) so it never reaches server logs or `Referer`.
Wrong/missing token ⇒ 404 `not_found` (indistinguishable from an unknown order).

## New error codes
| HTTP | code                          | meaning                                                   |
|------|-------------------------------|-----------------------------------------------------------|
| 409  | `order_already_paid`          | pay/verify on a paid order (verify is idempotent → 200)   |
| 409  | `order_expired`               | payment window closed; order cancelled, stock released    |
| 409  | `order_not_payable`           | order cancelled for another reason                        |
| 400  | `payment_verification_failed` | signature/amount/order mismatch — treat as unpaid         |
| 502  | `payment_gateway_error`       | Razorpay API failed/timed out — retry                     |
| 409  | `refund_exceeds_remaining`    | refund amount larger than what is left to refund          |

## Public endpoints

### `GET /storefront/config`
`{ "testMode": false, "onlinePaymentsEnabled": true, "razorpayKeyId": "rzp_live_…", "currency": "INR", "paymentExpiryMinutes": 30, "storeName": "Zhi-Mart Fireworks", "pricing": { "bulkMinOrder": 10000, "bulkDiscountPercent": 85, "standardDiscountPercent": 70 } }`
(`Cache-Control: public, max-age=60`). The key id is public by design; the secret never leaves the server.

### `POST /orders` (changed)
Request unchanged. 201 (replay 200):
```json
{
  "order": { "orderNumber": "ZM-261013-1001", "status": "pending_payment", "paymentExpiresAt": "…",
             "isTest": false, "pricingTier": "standard", "totalAmount": 1650, "mrpTotal": 3000, "itemCount": 10, "createdAt": "…",
             "items": [ … ] },
  "orderToken": "…43 chars…"
}
```

### `POST /orders/{orderNumber}/pay`  (header `X-Order-Token`)
Creates the Razorpay order on first call (`amount = total in paise`, `currency INR`,
`receipt = orderNumber`, `notes.order_id`), reuses it afterwards (Razorpay allows multiple
attempts per order). 200:
```json
{
  "keyId": "rzp_live_…", "razorpayOrderId": "order_…", "amountPaise": 165000, "currency": "INR",
  "name": "Zhi-Mart Fireworks", "description": "Order ZM-261013-1001",
  "prefill": { "name": "…", "email": "…|null", "contact": "+919840012345" },
  "notes": { "orderNumber": "ZM-261013-1001" },
  "paymentExpiresAt": "…", "testMode": false
}
```
Errors: 404, 409 `order_already_paid` / `order_expired` / `order_not_payable`, 502 `payment_gateway_error`.

### `POST /orders/{orderNumber}/pay/verify`  (header `X-Order-Token`)
Body (from Checkout's `handler`): `{ "razorpayOrderId": "order_…", "razorpayPaymentId": "pay_…", "razorpaySignature": "<hex>" }`
Server: constant-time `HMAC_SHA256(razorpayOrderId + "|" + razorpayPaymentId, KEY_SECRET)`;
order id must equal the order's stored Razorpay order; then `GET /v1/payments/{id}` must show
the same `order_id`, `amount == total`, `currency INR`, status `captured` (an `authorized`
payment is captured via the API). Marks the order paid → `pending_pack`. Idempotent.
200 `GuestOrder`. Errors: 400 `payment_verification_failed`, 404, 409 `order_expired`
(late payment refunded), 502.

### `GET /orders/{orderNumber}`  (header `X-Order-Token`)
200 `GuestOrder` (minimal PII):
```json
{ "orderNumber": "…", "status": "pending_payment|pending_pack|…|cancelled", "isTest": false,
  "pricingTier": "bulk|standard",
  "createdAt": "…", "paymentExpiresAt": "…|null",
  "payment": { "status": "pending|paid|refunded|failed", "instrument": "upi|card|…|null", "paidAt": "…|null" },
  "totalAmount": 1650, "mrpTotal": 3000, "itemCount": 10, "items": [ … ],
  "shipping": { "transportPartner": null, "docketNumber": null, "dispatchedAt": null, "deliveredAt": null },
  "customerName": "…", "city": "…" }
```

### `POST /webhooks/razorpay`
Configure in the Razorpay Dashboard → Webhooks with secret `RAZORPAY_WEBHOOK_SECRET` and
events `payment.authorized`, `payment.captured`, `payment.failed`, `order.paid`,
`refund.processed`, `refund.failed`. Verified with `X-Razorpay-Signature` = hex
`HMAC_SHA256(raw body, webhook secret)` (constant-time). Deduplicated by `X-Razorpay-Event-Id`
(`payment_events` primary key). Invalid signature ⇒ 400; 200 `{"status":"ok"}` once processed;
5xx on a transient failure (DB or Razorpay down) so Razorpay redelivers — an event is only treated as a
duplicate after it was processed successfully. Stored payloads are redacted (ids, amounts, statuses only). Not rate-limited per IP (signature
is the authentication); body ≤ 64 KiB.

## Admin additions
- `Order` gains `isTest`, `paymentExpiresAt`, and `payment.{instrument, razorpayOrderId, razorpayPaymentId, amountRefunded, attention}`; `status` may be `pending_payment`.
- `GET /admin/orders` excludes `pending_payment` orders unless `status=pending_payment` or `includeUnpaid=true`.
- `pending_payment` orders cannot be advanced via `/status` (409 `invalid_transition`); they may be cancelled (stock released).
- Manual `/payment` actions are rejected (409 `invalid_transition`) for `payment.method = "razorpay"`; use refunds.
- `POST /admin/orders/{id}/refund` `{ "amount": 1650 | null (full remaining), "reason": "…", "expectedVersion": 3 }`
  → Razorpay refund API (idempotent per request) → 200 `Order` (`payment.status = "refunded"` once fully refunded; partial refunds tracked in `amountRefunded`). Errors: 409 `invalid_transition` (not paid / nothing left), 502.
- `GET /admin/orders/{id}/payment-events` → `{ "events": [{ "id","event","outcome","receivedAt" }], "refunds": [{ "id","razorpayRefundId","amount","status","reason","adminEmail","createdAt" }] }`
- Dashboard: real revenue excludes `isTest` orders and unpaid (`pending_payment`) orders; adds
  `"test": { "orders": n, "revenue": x }` and `"unpaid": { "orders": n, "amount": x }`.

---

# Payment switch & admin order notifications (v1)

## `RAZORPAY_ENABLED` (default `false`)
| | `RAZORPAY_ENABLED=false` (launch default) | `RAZORPAY_ENABLED=true` |
|---|---|---|
| `POST /orders` creates | `status: "pending_pack"`, `payment: {method: "pay_later", status: "pending"}`, `paymentExpiresAt: null` | `status: "pending_payment"` + 30-min payment window (see Payments section) |
| Stock | deducted immediately, held until the admin cancels | reserved; released if unpaid at expiry |
| `POST /orders/{n}/pay`, `/pay/verify` | 409 `online_payments_disabled` | Razorpay flow |
| `POST /webhooks/razorpay` | 404 `not_found` | signature-verified processing |
| Payment collection | admin records it manually (`POST /admin/orders/{id}/payment` `mark_paid` with method + reference) | automatic |
| Admin notification email | queued when the order is **placed** | queued when the payment is **confirmed** (abandoned checkouts never email) |
| Razorpay keys | optional | required (validated at startup) |
| Admin Razorpay refund (`POST /admin/orders/{id}/refund`) | 409 `online_payments_disabled` for orders paid online before the switch — refund those from the Razorpay Dashboard; Razorpay is never called while disabled | Razorpay refund API |

`GET /storefront/config` gains `"onlinePaymentsEnabled": bool`; the storefront hides the Razorpay
flow and shows "Order received — we'll contact you to confirm payment and dispatch" when false.
`PAYMENT_TEST_MODE` (₹1 per unit) works in both modes.

`POST /orders` 201 response is unchanged in shape: `{ order: {...}, orderToken }` — in pay-later mode
`order.status` is `"pending_pack"` and `order.paymentExpiresAt` is `null`. The order page
(`GET /orders/{n}` with `X-Order-Token`) works in both modes.

## Admin "new order" email
- Recipients: `ORDER_NOTIFICATION_EMAILS` (comma-separated) if set, else every **active admin's login email**.
  Disable entirely with `ORDER_NOTIFICATION_ENABLED=false`.
- Delivery: transactional outbox (`email_outbox`, one row per order, inserted in the same DB transaction
  as the order/payment change) + background worker (every ~5 s, `SKIP LOCKED`, exponential backoff
  30 s → 1 h, parked as `failed` after 12 attempts). Sent via the configured provider (AWS SES; the
  `log` provider prints it in development).
- Content (rendered from current DB data at send time): subject
  `New order ZM-… — ₹X from <customer name>` (prefixed `[TEST] ` for test orders); order number and
  IST date/time; customer name, phone (tap-to-call), email, full address with pincode, notes; item table
  (product, pack, qty, unit price, line total); MRP total, savings, amount payable; pricing tier
  ("Bulk price (₹10,000+ order)" / "Standard price (order below ₹10,000)"); payment method/status
  (and Razorpay payment id when paid online); a button linking to the admin console
  (`PUBLIC_SITE_URL` + `/?page=admin`, omitted if unset). Every value is HTML-escaped.

---

# Customer order confirmation email (v1)

- **`customer.email` is now REQUIRED** on `POST /orders` (400 `validation_error`,
  message `customer.email is required …`). It receives the confirmation and tracking link.
- Sent through the same transactional outbox (`email_outbox` kind `customer_order_confirmation`), queued in the
  same transaction and at the same moment as the admin notification: on placement when
  `RAZORPAY_ENABLED=false`, on payment confirmation when `true`. Exactly one per order. Toggle with
  `CUSTOMER_ORDER_EMAIL_ENABLED` (default `true`).
- **Tracking link:** `{PUBLIC_SITE_URL}/?page=order&order=ZM-…#t=<orderToken>` (`Config::order_tracking_url`).
  The token stays in the URL fragment (never sent to servers/logs/Referer). `PUBLIC_SITE_URL` is required in
  production when customer emails are on; development falls back to `http://localhost:5173`.
- Recipient: only the order's customer email (one recipient per message). Subject:
  `Your Zhi-Mart order ZM-… is confirmed` (`[TEST] ` prefix for test orders).
- Content: greeting with the customer's name; order number + IST time; prominent **"Track your order"** button
  (plus the raw link for clients that block buttons) with a note to keep the link private; items
  (product, pack, qty, unit price, line total); MRP total, savings, total, pricing tier line; payment section — pay-later:
  "Our team will call you on +91 ••••• •1234 to confirm payment and dispatch", online: "Payment of ₹X received via UPI";
  delivery address; support line (WhatsApp/phone of the store if configured, otherwise "reply to this email"
  is NOT promised — no reply handling exists). Every value HTML-escaped; plain-text alternative.
