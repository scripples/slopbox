-- Join table: which vps_configs are available under each plan.
CREATE TABLE plan_vps_configs (
    plan_id       UUID NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    vps_config_id UUID NOT NULL REFERENCES vps_configs(id) ON DELETE CASCADE,
    PRIMARY KEY (plan_id, vps_config_id)
);
