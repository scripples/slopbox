-- Auth.js verification token (passwordless / email sign-in)
CREATE TABLE verification_token (
    identifier  TEXT NOT NULL,
    expires     TIMESTAMPTZ NOT NULL,
    token       TEXT NOT NULL,
    PRIMARY KEY (identifier, token)
);
