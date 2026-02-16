-- Auth.js-compatible tables for NextJS BFF authentication.

-- ── OAuth / credential accounts ────────────────────────────────────

CREATE TABLE accounts (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type                  TEXT NOT NULL,
    provider              TEXT NOT NULL,
    provider_account_id   TEXT NOT NULL,
    refresh_token         TEXT,
    access_token          TEXT,
    expires_at            INTEGER,
    token_type            TEXT,
    scope                 TEXT,
    id_token              TEXT,
    session_state         TEXT,
    UNIQUE(provider, provider_account_id)
);

CREATE INDEX idx_accounts_user_id ON accounts(user_id);

-- ── Sessions ───────────────────────────────────────────────────────

CREATE TABLE sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_token   TEXT NOT NULL UNIQUE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires         TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_sessions_user_id ON sessions(user_id);

-- ── Verification tokens (passwordless / email sign-in) ─────────────

CREATE TABLE verification_tokens (
    identifier  TEXT NOT NULL,
    token       TEXT NOT NULL,
    expires     TIMESTAMPTZ NOT NULL,
    UNIQUE(identifier, token)
);
