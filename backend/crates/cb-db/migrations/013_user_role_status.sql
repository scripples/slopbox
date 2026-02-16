CREATE TYPE user_role AS ENUM ('user', 'admin');
CREATE TYPE user_status AS ENUM ('pending', 'active', 'frozen');

ALTER TABLE users
    ADD COLUMN role   user_role   NOT NULL DEFAULT 'user',
    ADD COLUMN status user_status NOT NULL DEFAULT 'pending';
