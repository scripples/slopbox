-- Auth.js OAuth / credential accounts
CREATE TABLE accounts (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    "userId"              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type                  TEXT NOT NULL,
    provider              TEXT NOT NULL,
    "providerAccountId"   TEXT NOT NULL,
    refresh_token         TEXT,
    access_token          TEXT,
    expires_at            BIGINT,
    id_token              TEXT,
    scope                 TEXT,
    session_state         TEXT,
    token_type            TEXT,
    UNIQUE(provider, "providerAccountId")
);

CREATE INDEX idx_accounts_user_id ON accounts("userId");
