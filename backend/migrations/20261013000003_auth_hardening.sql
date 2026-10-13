-- Auth hardening (security review):
--  * per-(email, client IP bucket) login throttling that behaves identically for
--    unknown and known e-mail addresses (no enumeration, no global lock-out DoS);
--  * an account-wide failure ceiling for distributed attacks, which "trusted
--    devices" (a long-lived device cookie set after a full login) bypass;
--  * an index for the refresh_tokens.replaced_by self-reference (FK actions and
--    purges were doing a sequential scan per deleted row).

CREATE INDEX refresh_tokens_replaced_by_idx ON refresh_tokens (replaced_by);

-- key_hash = HMAC-SHA256(key derived from OTP_PEPPER, normalized_email || 0x00 || ip_bucket)
-- where ip_bucket is the IPv4 address or the IPv6 /64. Rows exist for any
-- attempted e-mail, registered or not.
CREATE TABLE login_throttles (
    key_hash      BYTEA       PRIMARY KEY,
    -- Attempts charged since the last lock (charged before the password/OTP
    -- check; refunded when the check succeeds).
    failure_count INTEGER     NOT NULL DEFAULT 0,
    -- Number of locks so far; each lock lasts base * 2^lock_level (capped at 24h).
    lock_level    INTEGER     NOT NULL DEFAULT 0,
    locked_until  TIMESTAMPTZ,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT login_throttles_key_len CHECK (octet_length(key_hash) = 32),
    CONSTRAINT login_throttles_nonneg CHECK (failure_count >= 0 AND lock_level >= 0)
);

CREATE INDEX login_throttles_updated_idx ON login_throttles (updated_at);

-- The legacy lockout columns now implement the account-wide ceiling:
-- failed_login_count counts failures (all clients) in the window starting at
-- failure_window_started_at; locked_until is the account-wide lock, which
-- trusted devices bypass. Reset any state recorded under the old semantics.
ALTER TABLE admins ADD COLUMN failure_window_started_at TIMESTAMPTZ;
UPDATE admins SET failed_login_count = 0, locked_until = NULL;

COMMENT ON COLUMN admins.failed_login_count IS
    'Failed password/OTP attempts across all clients in the current account-wide window';
COMMENT ON COLUMN admins.locked_until IS
    'Account-wide lock after the distributed-attack ceiling; trusted devices bypass it';

-- Devices that completed password + OTP. Only SHA-256 of the random device
-- cookie value is stored.
CREATE TABLE trusted_devices (
    id            UUID PRIMARY KEY,
    admin_id      UUID        NOT NULL REFERENCES admins (id) ON DELETE CASCADE,
    device_hash   BYTEA       NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL,
    ip            INET,
    user_agent    TEXT,
    last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT trusted_devices_hash_len CHECK (octet_length(device_hash) = 32)
);

CREATE UNIQUE INDEX trusted_devices_hash_key ON trusted_devices (device_hash);
CREATE INDEX trusted_devices_admin_idx ON trusted_devices (admin_id);
CREATE INDEX trusted_devices_expires_idx ON trusted_devices (expires_at);
