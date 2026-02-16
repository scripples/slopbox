-- Starter plan: single Hetzner CPX11 VPS config
--
-- CPX11: 2 shared AMD vCPU, 2 GB RAM, 40 GB SSD
-- Works across dev and prod — just run against the target database.

BEGIN;

-- Plan
INSERT INTO plans (name, max_agents, max_vpses, max_bandwidth_bytes, max_storage_bytes, max_cpu_ms, max_memory_mb_seconds)
VALUES ('starter', 1, 1, 10000000000, 40000000000, 0, 0)
ON CONFLICT (name) DO NOTHING;

-- VpsConfig (Hetzner CPX11)
INSERT INTO vps_configs (name, provider, image, cpu_millicores, memory_mb, disk_gb)
VALUES ('hetzner-cpx11', 'hetzner', 'ubuntu-24.04', 1000, 2048, 40)
ON CONFLICT (name) DO NOTHING;

-- Link config to plan
INSERT INTO plan_vps_configs (plan_id, vps_config_id)
SELECT p.id, vc.id
FROM plans p, vps_configs vc
WHERE p.name = 'starter' AND vc.name = 'hetzner-cpx11'
ON CONFLICT DO NOTHING;

COMMIT;
