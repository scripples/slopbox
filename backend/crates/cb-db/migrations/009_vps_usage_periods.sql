-- Period-based VPS usage metering (one row per VPS per calendar month).
CREATE TABLE vps_usage_periods (
    vps_id                 UUID NOT NULL REFERENCES vpses(id) ON DELETE CASCADE,
    period_start           DATE NOT NULL,
    bandwidth_bytes        BIGINT NOT NULL DEFAULT 0,
    cpu_used_ms            BIGINT NOT NULL DEFAULT 0,
    memory_used_mb_seconds BIGINT NOT NULL DEFAULT 0,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (vps_id, period_start)
);

CREATE TRIGGER trg_vps_usage_periods_updated_at
    BEFORE UPDATE ON vps_usage_periods
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
