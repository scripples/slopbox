-- Auth.js sessions (camelCase columns for @auth/pg-adapter)
CREATE TABLE sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    "sessionToken"  TEXT NOT NULL UNIQUE,
    "userId"        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires         TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_sessions_user_id ON sessions("userId");
