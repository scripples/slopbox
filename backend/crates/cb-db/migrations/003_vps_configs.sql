CREATE TABLE vps_configs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL UNIQUE,
    provider        TEXT NOT NULL,
    image           TEXT NOT NULL,
    cpu_millicores  INTEGER NOT NULL,
    memory_mb       INTEGER NOT NULL,
    disk_gb         INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_vps_configs_updated_at
    BEFORE UPDATE ON vps_configs
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
