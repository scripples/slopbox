CREATE TYPE vps_state AS ENUM ('provisioning', 'running', 'stopped', 'destroyed');

CREATE TABLE vpses (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    vps_config_id          UUID NOT NULL REFERENCES vps_configs(id),
    name                   TEXT NOT NULL,
    provider               TEXT NOT NULL,           -- 'fly', 'hetzner', etc.
    provider_vm_id         TEXT,                    -- remote VM id from provider
    address                TEXT,                    -- private IP / internal DNS
    state                  vps_state NOT NULL DEFAULT 'provisioning',
    storage_used_bytes     BIGINT NOT NULL DEFAULT 0,
    cpu_used_ms            BIGINT,                  -- cumulative, NULL for fixed-resource providers
    memory_used_mb_seconds BIGINT,                  -- cumulative, NULL for fixed-resource providers
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_vpses_user_id ON vpses(user_id);

CREATE TRIGGER trg_vpses_updated_at
    BEFORE UPDATE ON vpses
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
