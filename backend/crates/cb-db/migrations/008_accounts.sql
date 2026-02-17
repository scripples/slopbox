-- Auth.js OAuth / credential accounts (camelCase columns for @auth/pg-adapter)
CREATE TABLE accounts (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    "userId"              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type                  TEXT NOT NULL,
    provider              TEXT NOT NULL,
    "providerAccountId"   TEXT NOT NULL,
    "refreshToken"        TEXT,
    "accessToken"         TEXT,
    "expiresAt"           INTEGER,
    "tokenType"           TEXT,
    scope                 TEXT,
    "idToken"             TEXT,
    "sessionState"        TEXT,
    UNIQUE(provider, "providerAccountId")
);

CREATE INDEX idx_accounts_user_id ON accounts("userId");
