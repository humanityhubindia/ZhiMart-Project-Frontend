# Zhi-Mart backend

Rust (axum + hyper + sqlx/PostgreSQL) API for Zhi-Mart. It serves three things:

- **Public storefront:** the product catalog, plus guest checkout that reserves stock and
  takes payment online through Razorpay.
- **Admin console:** products, inventory, orders, payments and a dashboard.
- **Admin login:** password, then a 6-digit code emailed through AWS SES, then a
  short-lived JWT access token plus a rotating HttpOnly refresh cookie. Every session
  has a hard 30-day limit.

The endpoints, the error format and the token and cookie rules are in
**[API.md](API.md)**:
- [Admin Auth API](API.md#admin-auth-api-v1)
- [Catalog, Inventory & Orders API](API.md#catalog-inventory--orders-api-v1)
- [Payments (Razorpay) & Guest Orders](API.md#payments-razorpay--guest-orders-v1)
- [Payment switch & admin order notifications](API.md#payment-switch--admin-order-notifications-v1)

## How it fits together

```
browser ──HTTPS──► CDN / WAF ──► load balancer / reverse proxy (TLS ends here)
                                        │  X-Forwarded-For
                                        ▼
                          zhimart-backend (this binary, plain HTTP :8080)
                            ├─ edge: request-id, tracing, security headers, CORS,
                            │        real client IP, load shedding, timeouts,
                            │        body limit, per-IP rate limits
                            ├─ /api/v1/auth/*      login → OTP → session
                            ├─ /api/v1/products    public catalog (cacheable 10 s)
                            ├─ /api/v1/orders      guest checkout (stock reservation),
                            │                      pay / verify / status (X-Order-Token)
                            ├─ /api/v1/storefront/config
                            ├─ /api/v1/webhooks/razorpay   (HMAC-signed, not IP-limited)
                            ├─ /api/v1/admin/*     products, inventory, orders,
                            │                      payments, dashboard, sync (Bearer)
                            └─ /health/live, /health/ready
                                        │
                    PostgreSQL     AWS SES (OTP email)     Razorpay API
```

- **Login** (`POST /auth/login`): checks the password with Argon2id. Failed attempts
  count toward an account lockout. On success the server creates an OTP challenge and
  emails the code.
- **Verify** (`POST /auth/otp/verify`): checks the code against
  HMAC-SHA256(`OTP_PEPPER`, …). Each challenge allows only a few attempts. On success the
  server creates a session and returns an access token. It also sets the refresh cookie.
- **Refresh** (`POST /auth/refresh`) rotates the refresh token. If an old token is used
  again, the server treats it as theft and revokes the whole session.
- **Catalog and orders**: the database is the single source of truth. Prices always come
  from the database, never from the client. Placing an order decrements every line's
  stock in one transaction or does nothing. Admin edits use optimistic versioning, so a
  stale edit returns 409 `version_conflict`.
- **Background jobs**:
  - Every ~5 s: send queued admin "new order" emails from the outbox.
  - Every ~30 s: cancel unpaid orders past their payment window and release their stock.
  - Every ~10 minutes: delete expired challenges, tokens, sessions and old audit rows.
  - Every 60 s: remove idle entries from the rate limiters.

## Local development

Requirements: Rust ≥ 1.85 and Docker (or any local PostgreSQL 16).

```bash
cd backend
docker compose up -d db                 # Postgres 16 on 127.0.0.1:5433
cp .env.example .env
# Fill in JWT_SECRET and OTP_PEPPER (two different values):
openssl rand -base64 48
openssl rand -base64 48
# Online payments are OFF by default (RAZORPAY_ENABLED=false: pay-later orders), so no
# Razorpay keys are needed. To try Razorpay locally, see "Payments (Razorpay)".
# New-order emails to admins are printed to the log (EMAIL_PROVIDER=log).

cargo run -- migrate                                            # optional: `serve` runs migrations itself
cargo run -- create-admin --email you@example.com --name "Your Name"   # prompts for the password twice
cargo run                                                       # serves on 127.0.0.1:8080
```

With `EMAIL_PROVIDER=log` (the dev default) no email is sent. The code shows up in the
server log as `DEV ONLY — OTP for …`. Production refuses to start with this setting.

**Frontend.** In development the Vite dev server proxies `/api` to `127.0.0.1:8080`, so
the SPA and the API share one origin. This is required: the refresh cookie is
`SameSite=Strict`, so it is only sent on same-origin requests. Keep
`ALLOWED_ORIGINS=http://localhost:5173` anyway. Browsers still send that `Origin` header
on POST requests, and the CSRF check validates it.

**Other commands**

| Command | Purpose |
|---|---|
| `zhimart-backend [serve]` | Run the API (default). Runs migrations at startup unless `AUTO_MIGRATE=false`. |
| `zhimart-backend migrate` | Apply pending migrations and exit. |
| `zhimart-backend create-admin --email E --name N` | Create an admin. |
| `zhimart-backend reset-password --email E` | Set a new password, clear the lockout, revoke all of that admin's sessions. |

`migrate`, `create-admin` and `reset-password` need only `DATABASE_URL`. On a terminal,
the password is prompted twice and not echoed. Otherwise it is read as a single line
from stdin, e.g. `printf '%s\n' "$PW" | zhimart-backend create-admin …` from a secret
store. Passwords must meet the policy: at least 12 characters, using at least 3 of:
lowercase, uppercase, digit, symbol.

Tests: `cargo test` (unit tests need no database).

## Docker

```bash
docker build -t zhimart-backend .
docker compose --profile api up --build      # API + DB, local only
```

The image:
- Is multi-stage. The runtime image is `distroless/cc-debian12:nonroot`: no shell, and
  it runs as a non-root user.
- Has migrations compiled into the binary.
- Contains no secrets. All configuration comes from environment variables.
- Has no `HEALTHCHECK` (there is no curl in distroless). Configure probes in your
  orchestrator: liveness `GET /health/live`, readiness `GET /health/ready`.

## AWS SES setup

SES sends three kinds of mail:
- Admin sign-in codes.
- Admin "new order" notifications.
- Customer "order confirmed" emails with a private tracking link.

While the account is in the **SES sandbox**, mail is delivered only to verified
identities. **Production access (step 2 below) is mandatory once customer emails are
on**: you cannot verify every customer's address. While still in the sandbox, set
`CUSTOMER_ORDER_EMAIL_ENABLED=false`, or test only with verified addresses.

**Recommended: bounce and complaint handling.** Customer addresses are typed by
anonymous users, so some will bounce. SES suspends senders whose bounce rate exceeds
about 5% or whose complaint rate exceeds 0.1%. To stay safe:
- Create a **configuration set** with event publishing for Bounce, Complaint and
  Delivery to an **SNS topic** (or EventBridge), and set `SES_CONFIGURATION_SET`. All
  mail this app sends uses it.
- Subscribe an email or Slack alert to the topic, and watch the Reputation metrics in
  the SES console.
- Keep the account-level **suppression list** enabled for bounces and complaints. SES
  then stops sending to addresses that hard-bounced.

1. **Verify a domain identity** in your SES region, e.g. `zhimart.com`:
   - Turn on **Easy DKIM** (2048-bit) and publish the 3 CNAME records.
   - Configure a **custom MAIL FROM domain** (e.g. `mail.zhimart.com`): an MX record
     plus an SPF TXT record `v=spf1 include:amazonses.com ~all`. SPF then aligns with
     your domain.
   - Publish **DMARC**: `_dmarc.zhimart.com TXT "v=DMARC1; p=quarantine; rua=mailto:dmarc@zhimart.com"`.
     Tighten it to `p=reject` once reports look clean.
2. **Request production access**. New accounts are in the SES sandbox, which only
   delivers to verified addresses.
3. Optionally create a **configuration set**, publish bounce and complaint events to SNS
   or EventBridge, and set `SES_CONFIGURATION_SET`.
4. Set `EMAIL_PROVIDER=ses`, `AWS_REGION`, and
   `SES_FROM_ADDRESS="Zhi-Mart Admin <no-reply@zhimart.com>"`.
5. **Credentials**: prefer an ECS task role, EC2 instance profile or EKS IRSA over
   static keys. The SDK uses the standard credential chain, so no key variables are
   needed. Grant only this:

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "SendOtpMailOnly",
    "Effect": "Allow",
    "Action": "ses:SendEmail",
    "Resource": [
      "arn:aws:ses:REGION:ACCOUNT_ID:identity/zhimart.com",
      "arn:aws:ses:REGION:ACCOUNT_ID:configuration-set/CONFIG_SET_NAME"
    ],
    "Condition": { "StringEquals": { "ses:FromAddress": "no-reply@zhimart.com" } }
  }]
}
```

Drop the configuration-set ARN if you don't use one.

An SES send has a 9 s overall budget (4 s per attempt, at most 3 attempts), which keeps
it inside the request timeout. A failure returns `503 service_unavailable` to the client
and logs the SES error. The code itself is never logged.

## Production checklist

- [ ] **TLS ends at a proxy or load balancer** (ALB, nginx, Cloudflare). The app speaks
      plain HTTP and must not be reachable directly from the internet.
- [ ] **`TRUSTED_PROXIES` lists exactly that proxy's addresses/CIDRs.** Without it, every
      client appears as the proxy's IP, so everyone shares one rate-limit bucket. Never
      add client-reachable ranges: anyone in a trusted range can spoof `X-Forwarded-For`.
- [ ] `APP_ENV=production`, `COOKIE_SECURE=true`, `ALLOWED_ORIGINS=https://<admin origin>`
      (startup fails if these are unsafe).
- [ ] `JWT_SECRET`, `OTP_PEPPER` and `DATABASE_URL` come from a secret manager (AWS
      Secrets Manager / SSM), not from files in the image or repository.
- [ ] Database: TLS (`sslmode=verify-full`), a dedicated least-privilege role, and
      backups. `DB_MAX_CONNECTIONS × replicas` must fit the server's `max_connections`.
- [ ] Liveness probe on `/health/live`, readiness probe on `/health/ready`. On
      SIGTERM, readiness switches to 503 and in-flight requests get 20 s to finish. In
      Kubernetes, add a short `preStop` sleep so endpoints are removed before shutdown
      starts.
- [ ] Allow at least `MAX_CONNECTIONS` (10 000) + DB pool + headroom file descriptors
      (`ulimit -n`; Docker's default is fine).
- [ ] Behind a CDN/proxy, check that its cache honours the API's `Cache-Control`.
      `GET /api/v1/products` may be cached for 10 s; everything else under `/api` is
      `no-store` and must never be cached.
- [ ] Ship the JSON logs and alert on:
      - `level=ERROR`
      - "overloaded: concurrency limit reached"
      - "rate limiter tracking an unusually large number of clients"
      - SES send failures
      - "payment expiry: sweep failed"
      - Razorpay gateway errors (`payment_gateway_error`)
      - non-null `payment_attention`
      - "order notification permanently failed" / "send failed"
- [ ] Create the first admin with `create-admin`, then sign in once to confirm SES
      delivery end to end.
- [ ] Order emails:
      - `ORDER_NOTIFICATION_EMAILS` is set, or every active admin should receive them.
      - `PUBLIC_SITE_URL` is set: it is required for customer tracking links and is
        used by the admin console link.
      - SES has **production access**, which customer emails require.
      - A configuration set with SNS bounce/complaint notifications is configured.
      - Place a test order: confirm the admin email arrives, and that the customer's
        tracking link opens the order page.
- [ ] Razorpay (only once `RAZORPAY_ENABLED=true`):
      - Live keys and webhook secret are in the secret manager.
      - The **Live** webhook is configured with all six events.
      - Automatic capture is enabled.
      - `PAYMENT_TEST_MODE=false` (or, temporarily, `true` for a real ₹1 smoke test that
        you then refund).

### Rate limits at a glance

All limits are per client IP; IPv6 is grouped by /64.

| Scope | Limit | Notes |
|---|---|---|
| Everything under `/api` | `RATE_LIMIT_GLOBAL_PER_MINUTE` (300/min, burst 75) | Includes 404s. |
| `POST /auth/login`, `/auth/otp/verify`, `/auth/otp/resend` | `RATE_LIMIT_AUTH_PER_MINUTE` (10/min, burst 5) | Account lockout and OTP attempt caps are also enforced in the DB. |
| `POST /auth/refresh`, `GET /auth/me` | 60/min, burst 20 | |
| `POST /auth/logout` | global limit only | **Exempt** from the session limit: a rate-limited logout would leave the session alive while the user believes they signed out. |
| `POST /orders` | 6/min, burst 3 | See below. |
| `GET /storefront/config`, `GET /orders/{n}`, `POST /orders/{n}/pay`, `POST /orders/{n}/pay/verify` | 40/min, burst 15 | Covers the order status page polling every ~3 s, plus a second tab. A valid `X-Order-Token` is also required. |
| `POST /webhooks/razorpay` | 600/min, burst 100 (safety valve only) | **Outside** the global limit: the HMAC signature is its authentication, and Razorpay delivers in bursts from a few IPs. Body limit, timeout and load shedding still apply. |
| `/health/*` | none | Never load-shed. Readiness is cached and costs at most one DB ping per second. |

One admin with a few dashboard tabs fits comfortably in the global limit. Each visible tab
polls `GET /admin/sync` every ~5 s and refetches lists only on change, so the worst case is
about 36 req/min per tab. If several staff share one office NAT address, raise
`RATE_LIMIT_GLOBAL_PER_MINUTE`.

### Order abuse protections

Checkout is public and reserves real stock, so it is the most abusable endpoint. The
protections are layered:
1. **`orders` rate limiter**: 6/min per IP with a burst of 3, on top of the global limit.
2. **Per-IP and per-phone caps, enforced in PostgreSQL.** At most 10 orders per IP per
   rolling hour and 5 `pending_pack` orders per phone number per rolling 24 h (429 with an
   exact `Retry-After`). Because they live in the database, they hold across replicas and
   restarts, and advisory locks make them race-free.
3. **Idempotency keys.** The SPA generates one UUID per checkout attempt. A retry
   returns the original order (200) instead of reserving stock twice. Reusing a key with
   a different payload returns 409 `idempotency_conflict`.
4. **Strict validation.** At most 50 lines, quantity 1–1000, body ≤ `MAX_BODY_BYTES`
   (64 KiB). Prices are recomputed server-side.
5. **Admin recovery.** Cancelling an order before dispatch atomically restocks every line.

Bulk fake orders from many IPs are an edge/WAF problem. Watch the dashboard's
`pending_pack` count, and consider a WAF rate rule on `POST /api/v1/orders`.

### Operations: inventory

- **Stock ledger.** Stock lives in `products.stock`. Every change is recorded in
  `stock_movements` with `delta`, `stock_after`, the reason, and the order or admin
  involved. Reasons: `initial`, `order_placed`, `order_cancelled`, `payment_expired`,
  `payment_revived`, `manual_set`, `manual_adjust`, `restock`. The admin UI shows this ledger per product. When
  reconciling a physical count, use **Set**, which requires the stock value the admin
  saw and returns 409 if an order landed meanwhile. Use **Add** for restocks and
  **Remove** for write-offs, rather than editing rows by hand.
- **Never `UPDATE products SET stock = …` directly**: that bypasses the ledger.
- **Products are archived, not deleted**: they disappear from the storefront but keep
  their order history. Use restore to bring one back.
- **Reconciliation check.** Each product's latest `stock_after` should equal its
  `stock`:
  ```sql
  SELECT p.id, p.stock, m.stock_after
  FROM products p
  JOIN LATERAL (SELECT stock_after FROM stock_movements
                WHERE product_id = p.id ORDER BY id DESC LIMIT 1) m ON true
  WHERE m.stock_after <> p.stock;
  ```

## Order payments: pay-later vs online

`RAZORPAY_ENABLED` decides how customers pay.

**`false` (the default, and the recommended launch setting): pay-later**
- `POST /orders` creates the order as `pending_pack`, with payment `pay_later` /
  `pending`. Stock is deducted immediately.
- The admins receive the "new order" email right away.
- The storefront shows "Order received — we'll contact you to confirm payment and
  dispatch".
- Staff contact the customer and collect payment (UPI, bank transfer, cash). They then
  record it in the admin console with **Mark paid** (method plus reference such as the
  UTR).
- If the customer never pays, **Cancel** the order: this restocks every line.
- The payment and webhook endpoints answer `409 online_payments_disabled` and `404`.
- No Razorpay keys are needed.

**`true`: Razorpay**
- Online payment, with a stock reservation and an expiry window. Described below.
- The admin email is sent only once a payment is **confirmed**, so abandoned checkouts
  never email anyone.
- The keys are required and validated at startup.

`PAYMENT_TEST_MODE` (₹1 per unit) works in both modes.

## Pricing: bulk vs standard

Two price tiers, decided per order, server-side:

- **Bulk.** The cart's total at **price-list prices** (`products.price`, about 85 % off
  MRP) is at least `BULK_MIN_ORDER_RUPEES` (default ₹10,000; exactly ₹10,000 counts).
  Every line is charged its list price.
- **Standard.** Below that minimum, **every** line is charged the standard price:
  MRP × (100 − `STANDARD_DISCOUNT_PERCENT`) / 100 (default 75 % off), rounded **up** to
  a whole rupee, never below the list price and never above the MRP. Example: MRP ₹210,
  list ₹32 → standard ₹53 (₹52.50 rounded up); MRP ₹200 → ₹50. A product discounted
  less than 75 % (or without an MRP) costs its list price in both tiers.

The tier is decided on list prices, so a ₹9,990 list-price cart pays standard prices
(which may come to more than ₹10,000) and adding one more item can lower the bill. The
storefront shows both prices (`price`, `standardPrice`) and the rule (`pricing`), so it can
tell the customer how far they are from bulk pricing.

The tier an order was charged at is stored in `orders.pricing_tier` (`bulk` | `standard`;
orders placed before this feature are `bulk`) and appears as `pricingTier` in the order
JSON and as a "Pricing" line in both order emails. `BULK_DISCOUNT_PERCENT` (default 85)
is only a label for the storefront. Under `PAYMENT_TEST_MODE` the tier is still recorded
from real list prices, but every unit is charged ₹1.

Changing these settings affects new orders only; existing orders keep the prices they were
placed at.

### Delivery charge

After tier pricing, the goods total is the order's **subtotal**. An order whose subtotal
is below `FREE_DELIVERY_MIN_RUPEES` (default ₹5,000) pays a flat `DELIVERY_FEE_RUPEES`
(default ₹350); from ₹5,000 (inclusive) delivery is free. `FREE_DELIVERY_MIN_RUPEES=0`
makes delivery always free; `DELIVERY_FEE_RUPEES=0` does too.

- The server decides the fee; the client never sends it. It is stored per order in
  `orders.delivery_fee_paise` (orders placed before this feature have 0).
- `orders.total_paise` = sum of the line totals + `delivery_fee_paise`. It stays the
  payable amount, so the Razorpay amount, payment verification, refunds (a full refund
  returns the delivery charge too), manual "mark paid" and dashboard revenue all include
  delivery. Top-product revenue is line totals, so it excludes it.
- Order JSON carries `subtotal`, `deliveryFee` and `totalAmount`; `GET /products` and
  `GET /storefront/config` publish `pricing.deliveryFee` and `pricing.freeDeliveryMin`.
- Both order emails show Subtotal, Delivery ("₹350" or "FREE"), "Tax (GST): ₹0" (prices
  are tax-inclusive) and the total. Savings are computed on the goods only.
- Under `PAYMENT_TEST_MODE` the delivery charge is ₹1 (like every unit), applied when the
  ₹1-per-unit subtotal is below the free-delivery minimum.
- Because a small order plus delivery can cost more than its MRP, the database checks
  `mrp_total_paise >= total_paise - delivery_fee_paise` (goods only).

### Checkout address

The store delivers within Tamil Nadu only, so the storefront no longer asks for a state.
`customer.state` is optional on `POST /orders`; missing or blank means "Tamil Nadu".

## Customer order confirmation emails

`CUSTOMER_ORDER_EMAIL_ENABLED` defaults to `true`. `customer.email` is required at
checkout.

- **When it is sent**: through the same outbox, at the same moment as the admin email:
  on placement for pay-later orders, or on payment confirmation with Razorpay. Exactly
  one email per order.
- **Recipient**: only the order's customer, one recipient per message. If the address
  is missing or invalid, the outbox row is marked `failed` with reason
  "no customer email".
- **Content**:
  - Built for phones: single column, text 14 px or larger, a large
    **Track your order** button, plus the raw link for clients that block buttons.
  - The tracking link is `{PUBLIC_SITE_URL}/?page=order&order=ZM-…#t=<token>`. The
    token sits in the URL fragment, so it never reaches servers, logs or `Referer`.
    The email warns: "keep this link private — anyone with it can see your order
    status".
  - Items, MRP total, savings, subtotal, delivery ("₹350" or "FREE"), "Tax (GST): ₹0"
    and total, then the delivery address.
  - Payment section:
    - Pay-later: "Our team will call you on +91 ••••• •1234…". The phone is masked to
      its last 4 digits.
    - Online: "Payment of ₹X received via UPI".
  - Test orders get a `[TEST] ` subject prefix and a banner.
  - No reply handling is promised: the footer says the mailbox is not monitored.
- **Privacy in logs**: the worker logs only the order number and email kind. It never
  logs the tracking URL or token, and never logs the customer's address; any address
  in a provider error is redacted. `last_error` in the database never contains an
  address.
- **`PUBLIC_SITE_URL` is required in production** while customer emails are on: startup
  fails without it. In development it falls back to `http://localhost:5173`.
- **Turning it off**: with `CUSTOMER_ORDER_EMAIL_ENABLED=false`, no new customer rows
  are queued. Rows already queued are left pending without spending an attempt, and go
  out if the flag is turned back on. The same applies to admin rows when
  `ORDER_NOTIFICATION_ENABLED=false`.

## Admin "new order" emails

- **Recipients**: `ORDER_NOTIFICATION_EMAILS` (comma-separated, max 20). When unset,
  every **active** admin's login email.
- **Turning them off**: `ORDER_NOTIFICATION_ENABLED=false` stops sending entirely.
- **Delivery is reliable and never duplicated per order**:
  - The order (or payment confirmation) and an `email_outbox` row are written in the
    same database transaction.
  - A background worker on every replica polls about every 5 s, using
    `SKIP LOCKED` plus a 120 s lease.
  - It renders the email from current order data and sends it through SES.
  - On failure it retries with exponential backoff (30 s, doubling up to 1 h), then
    parks the row as `failed` after 12 attempts.
  - Provider errors are logged in full; `last_error` keeps only a short reason with no
    personal data.
- **Content**:
  - Subject `New order ZM-… — ₹X from <customer>`, prefixed `[TEST] ` for test orders.
  - Order number and time in IST.
  - Customer details: name, tap-to-call phone, email, address with pincode, notes.
  - An item table with MRP, savings, subtotal, delivery ("₹350" or "FREE"),
    "Tax (GST): ₹0" and the amount payable.
  - Payment method and status, plus the Razorpay payment id when paid online.
  - An **Open admin console** button pointing at `PUBLIC_SITE_URL/?page=admin` (only
    when that is set).
  - Every customer value is HTML-escaped.
- **Development**: with `EMAIL_PROVIDER=log` the text version of each email is printed
  to the server log.
- **Ops queries**:
  - `SELECT status, count(*) FROM email_outbox GROUP BY 1;`
  - Stuck or failed rows:
    `SELECT o.order_number, e.attempts, e.last_error FROM email_outbox e JOIN orders o ON o.id = e.order_id WHERE e.status <> 'sent' ORDER BY e.created_at DESC;`
  - Resend a failed row:
    `UPDATE email_outbox SET status = 'pending', attempts = 0, next_attempt_at = now() WHERE id = '…';`
  - Sent rows are purged after 90 days, failed rows after a year.

## Payments (Razorpay)

Everything in this section applies only when `RAZORPAY_ENABLED=true`. Launch with
`false`, then turn it on once the keys, the webhook and automatic capture are configured.

Customers check out as guests and pay online through Razorpay Checkout (UPI, cards,
netbanking, wallets). **The browser never decides that an order is paid.** An order
becomes paid only in one of two ways:
- The server verifies the Checkout signature **and** confirms the payment with the
  Razorpay API: order id, amount, currency and status must all match.
- The server receives a signature-verified webhook.

Both paths run the same idempotent code. The endpoint details are in
[API.md](API.md#payments-razorpay--guest-orders-v1).

### 1. API keys
Dashboard → **Account & Settings → API Keys** → Generate Key.
- **Test keys** (`rzp_test_…`): the dashboard's Test Mode toggle. No real money moves; use
  Razorpay's test cards and UPI ids.
- **Live keys** (`rzp_live_…`): available after KYC and activation. Real payments.

Set `RAZORPAY_KEY_ID` and `RAZORPAY_KEY_SECRET`. The secret is shown only once: store it
in your secret manager. The key id is public, because the browser needs it to open
Checkout.

### 2. Webhook
Dashboard → **Account & Settings → Webhooks** → Add New Webhook. Configure it
separately for Test and Live mode.
- **URL:** `https://<api-host>/api/v1/webhooks/razorpay`
- **Secret:** the value of `RAZORPAY_WEBHOOK_SECRET`. Generate it with
  `openssl rand -hex 32`, and make it different from the key secret.
- **Active events:** `payment.authorized`, `payment.captured`, `payment.failed`,
  `order.paid`, `refund.processed`, `refund.failed`.

Webhooks are the safety net for customers who close the tab after paying. Without them,
those orders depend on the expiry sweep plus manual reconciliation. Every delivery is
stored in `payment_events` and deduplicated by event id, so redeliveries are harmless.
A bad signature gets 400; Razorpay retries any non-2xx response.

### 3. Automatic capture
Dashboard → **Account & Settings → Payment Capture** → set **automatic capture**. Use
an expiry that is at least `PAYMENT_EXPIRY_MINUTES`. As a backstop, the server also
captures an `authorized` payment when it verifies it, but automatic capture prevents
payments from sitting uncaptured and being auto-refunded by Razorpay.

### 4. Test mode: ₹1 per unit
`PAYMENT_TEST_MODE=true` charges **₹1 per unit for every product**. This is enforced
server-side:
- The storefront shows `testMode`, and the real MRP is kept for display.
- The delivery charge (when it applies) is ₹1 too; `pricing.deliveryFee` reports 1.
- Orders are flagged `isTest` and excluded from revenue on the dashboard.

| Keys | `PAYMENT_TEST_MODE` | Result |
|---|---|---|
| `rzp_test_…` | `true` or `false` | Simulated payments only; no money moves. |
| `rzp_live_…` | `true` | **Real ₹1 payments**: an end-to-end check of the live setup with real UPI or cards. Refund them from the admin console afterwards. |
| `rzp_live_…` | `false` | Production: real prices. |

With `APP_ENV=production`, `RAZORPAY_ENABLED=true` and test mode off, the server refuses
to start unless the keys are live. At startup it logs a warning when test mode is on, and whether the keys are
test or live. Secrets are never logged.

### 5. Order and payment lifecycle
```
POST /orders ─► pending_payment  (stock reserved, payment window = PAYMENT_EXPIRY_MINUTES)
   POST /orders/{n}/pay ─► Razorpay order created once, reused on retries ─► Checkout
   verify endpoint or webhook ─► payment confirmed ─► pending_pack ─► dispatched ─► … ─► delivered
   window passes unpaid ─► expiry sweep (every ~30 s) ─► cancelled, stock released
   payment arrives after expiry ─► stock re-reserved if still available ("revived"),
                                   otherwise refunded automatically
```
The storefront passes the guest order token in `X-Order-Token`. The token lives in the
URL fragment, so it never reaches server logs or `Referer` headers.

### 6. Refunds
In the admin console, open the order and choose **Refund** (full or partial). This calls
the Razorpay refund API, with each request idempotent. The result arrives through the
`refund.processed` / `refund.failed` webhooks and is shown in the order's payment
events. Manual "mark paid/refunded" actions are blocked for Razorpay orders. If an
automatic refund of a late payment fails, the order gets `payment_attention` and is
highlighted in the console: refund it from the Razorpay dashboard, then note it on the
order.

### 7. Reconciliation
- Daily: compare the Razorpay dashboard's captured payments and settlements with paid
  orders:
  ```sql
  SELECT order_number, razorpay_order_id, razorpay_payment_id, total_paise,
         amount_refunded_paise, paid_at
  FROM orders
  WHERE payment_method = 'razorpay' AND NOT is_test AND paid_at >= now() - interval '1 day'
  ORDER BY paid_at;
  ```
- Orders that need a human:
  `SELECT order_number, payment_attention FROM orders WHERE payment_attention IS NOT NULL;`
- Webhook health. Events that were received but never processed point to a processing
  error; check the logs by event id:
  `SELECT id, event, received_at, outcome FROM payment_events WHERE processed_at IS NULL ORDER BY received_at DESC LIMIT 50;`
- Refunds not yet confirmed:
  `SELECT * FROM refunds WHERE status = 'pending' AND created_at < now() - interval '1 hour';`
- A payment in Razorpay with no matching paid order usually means the customer paid
  after expiry. That case is revived or auto-refunded; look up its
  `razorpay_payment_id` in `payment_events`.

### 8. Frontend Content-Security-Policy
If the storefront sends a CSP, it must allow Razorpay Checkout. Check this against
Razorpay's current documentation:
```
script-src  'self' https://checkout.razorpay.com;
frame-src   https://api.razorpay.com https://*.razorpay.com;
connect-src 'self' https://api.razorpay.com https://lumberjack.razorpay.com;
img-src     'self' data: https://pub-a53dd3b873894b56842f46208c52fcd1.r2.dev;
```
`img-src` covers the product photos on Cloudflare R2 (`R2_PUBLIC_URL`). Swap the host if
the bucket moves to a custom domain.
The API's own `Content-Security-Policy: default-src 'none'` applies only to JSON
responses and does not affect the storefront.

### DDoS and abuse: what the app does and doesn't do

The app protects **itself and the database**:
- Per-IP GCRA rate limits (see the table above). IPv6 clients are grouped by /64.
- A cap on in-flight requests. Excess requests are shed with an immediate 503 instead of
  queueing.
- Request timeouts and a 64 KiB body limit (`MAX_BODY_BYTES`).
- Slowloris defences: a 10 s deadline for request headers and for the first bytes on a
  new connection.
- HTTP/2 limits on concurrent streams and header-list size.
- A cap of 10 000 open connections. At the cap the server stops calling `accept()` rather
  than running out of memory.
- A cap of **64 open connections per client IP** (IPv6 grouped by /64). Excess
  connections are closed immediately, so one host cannot take every slot. Peers in
  `TRUSTED_PROXIES` are exempt, because every proxied connection comes from them; behind
  a proxy, enforce per-client connection limits there.
- An idle timeout: a connection with no request in flight for 60 s is closed gracefully
  (an HTTP/2 GOAWAY). hyper has no HTTP/2 idle timeout of its own, and keep-alive pings
  would otherwise hold idle connections forever. Idle HTTP/1 keep-alive connections
  already close after 10 s.
- Bounded Argon2 concurrency.

The app **cannot absorb network- or transport-layer floods**: SYN floods, UDP
reflection, or HTTP floods larger than the instance's bandwidth or CPU. Those must be
stopped before traffic reaches it. Put it behind **Cloudflare**, or **AWS WAF + Shield**
on CloudFront/ALB, with:
- A WAF rate-based rule on `/api/v1/auth/*`.
- Managed bot and IP-reputation rules.
- Ideally, the admin hostname restricted by IP allow-list or Cloudflare Access.

**Scaling out:** rate limits are kept **per process**. With N replicas, a client gets up
to N× the configured budget. For a strict global budget, use one of:
- Rate limiting at the edge (WAF / Cloudflare / API gateway). This is the preferred
  option.
- A shared store such as Redis behind the same `IpRateLimiter` interface.

Account lockout and OTP attempt limits are stored in PostgreSQL, so they are already
global across replicas.

### Key rotation

- **`JWT_SECRET`**: rotating it invalidates every outstanding access token immediately.
  Refresh tokens are opaque and stored in the database, so clients silently get new
  access tokens through `/auth/refresh`. Rotation alone does **not** sign everyone out.
  To force a sign-out, also revoke all sessions (`UPDATE sessions SET revoked_at = now(),
  revoke_reason = 'key_rotation' WHERE revoked_at IS NULL;`). Rotate the secret
  immediately if it may have leaked.
- **`OTP_PEPPER`**: rotating it only invalidates sign-in codes currently in flight (valid
  for 5 minutes at most). It is safe to rotate at any time.
- **Database / AWS credentials**: rotate them in the secret manager and restart or
  redeploy. With IAM roles there is nothing to rotate.
