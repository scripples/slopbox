-- Rename auth table columns from snake_case to camelCase
-- to match @auth/pg-adapter v1.7+ expectations.

-- accounts
ALTER TABLE accounts RENAME COLUMN user_id TO "userId";
ALTER TABLE accounts RENAME COLUMN provider_account_id TO "providerAccountId";
ALTER TABLE accounts RENAME COLUMN refresh_token TO "refreshToken";
ALTER TABLE accounts RENAME COLUMN access_token TO "accessToken";
ALTER TABLE accounts RENAME COLUMN expires_at TO "expiresAt";
ALTER TABLE accounts RENAME COLUMN token_type TO "tokenType";
ALTER TABLE accounts RENAME COLUMN id_token TO "idToken";
ALTER TABLE accounts RENAME COLUMN session_state TO "sessionState";

-- sessions
ALTER TABLE sessions RENAME COLUMN session_token TO "sessionToken";
ALTER TABLE sessions RENAME COLUMN user_id TO "userId";
