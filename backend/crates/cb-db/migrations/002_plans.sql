CREATE TABLE plans (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                   TEXT NOT NULL UNIQUE,
    max_agents             INTEGER NOT NULL,
    max_vpses              INTEGER NOT NULL,
    max_bandwidth_bytes    BIGINT NOT NULL DEFAULT 107374182400,   -- 100 GB
    max_storage_bytes      BIGINT NOT NULL DEFAULT 53687091200,    -- 50 GB
    max_cpu_ms             BIGINT NOT NULL DEFAULT 360000000,      -- 100 CPU-hours
    max_memory_mb_seconds  BIGINT NOT NULL DEFAULT 1843200000,     -- ~512 MB steady for 1 month
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_plans_updated_at
    BEFORE UPDATE ON plans
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
