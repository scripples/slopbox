-- Add location column to vps_configs
ALTER TABLE vps_configs ADD COLUMN location TEXT;

-- Make image nullable (snapshots may inherit from provider defaults)
ALTER TABLE vps_configs ALTER COLUMN image DROP NOT NULL;

-- Remove redundant provider column from vpses (derivable via vps_config_id → vps_configs.provider)
ALTER TABLE vpses DROP COLUMN provider;
