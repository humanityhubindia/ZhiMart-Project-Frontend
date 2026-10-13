-- Admin authentication schema: password + email OTP (2FA), rotating refresh tokens
-- bound to a session with a hard 30-day absolute lifetime.

CREATE TABLE admins (
    id                  UUID PRIMARY KEY,
    email               TEXT        NOT NULL,
    password_hash       TEXT        NOT NULL,
    display_name        TEXT        NOT NULL,
    is_active           BOOLEAN     NOT NULL DEFAULT TRUE,
    failed_login_count  INTEGER     NOT NULL DEFAULT 0,
    locked_until        TIMESTAMPTZ,
    last_login_at       TIMESTAMPTZ,
    password_changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT admins_email_lowercase CHECK (email = lower(email)),
    CONSTRAINT admins_email_length CHECK (char_length(email) BETWEEN 3 AND 254),
    CONSTRAINT admins_failed_login_nonneg CHECK (failed_login_count >= 0)
);

CREATE UNIQUE INDEX admins_email_key ON admins (email);

-- One row per "password accepted, waiting for the emailed code" step.
-- The code itself is never stored: only HMAC-SHA256(pepper, challenge_id || code).
CREATE TABLE otp_challenges (
    id              UUID PRIMARY KEY,
    admin_id        UUID        NOT NULL REFERENCES admins (id) ON DELETE CASCADE,
    code_hash       BYTEA       NOT NULL,
    expires_at      TIMESTAMPTZ NOT NULL,
    attempts        INTEGER     NOT NULL DEFAULT 0,
    max_attempts    INTEGER     NOT NULL,
    resend_count    INTEGER     NOT NULL DEFAULT 0,
    last_sent_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    consumed_at     TIMESTAMPTZ,
    invalidated_at  TIMESTAMPTZ,
    ip              INET,
    user_agent      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT otp_attempts_nonneg CHECK (attempts >= 0),
    CONSTRAINT otp_code_hash_len CHECK (octet_length(code_hash) = 32)
);

CREATE INDEX otp_challenges_admin_created_idx ON otp_challenges (admin_id, created_at DESC);
CREATE INDEX otp_challenges_expires_idx ON otp_challenges (expires_at);

-- A login session (refresh-token family). absolute_expires_at is fixed at
-- creation (created_at + 30 days) and is never extended by token rotation.
CREATE TABLE sessions (
    id                  UUID PRIMARY KEY,
    admin_id            UUID        NOT NULL REFERENCES admins (id) ON DELETE CASCADE,
    absolute_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at          TIMESTAMPTZ,
    revoke_reason       TEXT,
    ip                  INET,
    user_agent          TEXT,
    last_used_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT sessions_expiry_after_creation CHECK (absolute_expires_at > created_at)
);

CREATE INDEX sessions_admin_idx ON sessions (admin_id);
CREATE INDEX sessions_expires_idx ON sessions (absolute_expires_at);

-- Opaque refresh tokens; only SHA-256(token) is stored. Each refresh rotates
-- the token: the old row gets used_at/replaced_by set. Presenting a token whose
-- used_at is already set is treated as theft and revokes the whole session.
CREATE TABLE refresh_tokens (
    id           UUID PRIMARY KEY,
    session_id   UUID        NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    token_hash   BYTEA       NOT NULL,
    expires_at   TIMESTAMPTZ NOT NULL,
    used_at      TIMESTAMPTZ,
    replaced_by  UUID REFERENCES refresh_tokens (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT refresh_token_hash_len CHECK (octet_length(token_hash) = 32)
);

CREATE UNIQUE INDEX refresh_tokens_hash_key ON refresh_tokens (token_hash);
CREATE INDEX refresh_tokens_session_idx ON refresh_tokens (session_id);
CREATE INDEX refresh_tokens_expires_idx ON refresh_tokens (expires_at);

-- Append-only security audit trail.
CREATE TABLE auth_audit_log (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    admin_id    UUID REFERENCES admins (id) ON DELETE SET NULL,
    event       TEXT        NOT NULL,
    ip          INET,
    user_agent  TEXT,
    metadata    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX auth_audit_log_admin_created_idx ON auth_audit_log (admin_id, created_at DESC);
CREATE INDEX auth_audit_log_created_idx ON auth_audit_log (created_at);
