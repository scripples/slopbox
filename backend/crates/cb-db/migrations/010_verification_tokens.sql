-- Auth.js verification tokens (passwordless / email sign-in)
CREATE TABLE verification_tokens (
    identifier  TEXT NOT NULL,
    token       TEXT NOT NULL,
    expires     TIMESTAMPTZ NOT NULL,
    UNIQUE(identifier, token)
);
