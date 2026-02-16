ALTER TABLE plans
    ADD COLUMN overage_bandwidth_cost_per_gb_cents  BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN overage_cpu_cost_per_hour_cents       BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN overage_memory_cost_per_gb_hour_cents BIGINT NOT NULL DEFAULT 0;
