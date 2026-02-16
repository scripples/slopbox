-- Seed data for demo deployment

-- VpsConfig for Sprites
INSERT INTO vps_configs (name, provider, image, cpu_millicores, memory_mb, disk_gb)
VALUES ('sprites-standard', 'sprites', 'ubuntu-25', 1000, 512, 10)
ON CONFLICT DO NOTHING;

-- VpsConfig for Hetzner (snapshot-based: image field holds snapshot ID, set via env/SQL)
-- Uses cpx11 (2 vCPU, 2 GB RAM) with a pre-baked snapshot containing Docker + OpenClaw
INSERT INTO vps_configs (name, provider, image, cpu_millicores, memory_mb, disk_gb)
VALUES ('hetzner-standard', 'hetzner', 'ubuntu-24.04', 2000, 2048, 20)
ON CONFLICT DO NOTHING;

-- Demo plan with generous limits (5 agents, 5 VPSes, 50 GB bandwidth)
INSERT INTO plans (
    name, max_agents, max_vpses,
    max_bandwidth_bytes, max_storage_bytes, max_cpu_ms, max_memory_mb_seconds,
    overage_bandwidth_cost_per_gb_cents, overage_cpu_cost_per_hour_cents, overage_memory_cost_per_gb_hour_cents
) VALUES (
    'demo', 5, 5,
    53687091200, 10737418240, 999999999, 999999999,
    0, 0, 0
) ON CONFLICT DO NOTHING;

-- Link demo plan to both VpsConfigs
INSERT INTO plan_vps_configs (plan_id, vps_config_id)
SELECT p.id, vc.id
FROM plans p, vps_configs vc
WHERE p.name = 'demo' AND vc.name = 'sprites-standard'
ON CONFLICT DO NOTHING;

INSERT INTO plan_vps_configs (plan_id, vps_config_id)
SELECT p.id, vc.id
FROM plans p, vps_configs vc
WHERE p.name = 'demo' AND vc.name = 'hetzner-standard'
ON CONFLICT DO NOTHING;
