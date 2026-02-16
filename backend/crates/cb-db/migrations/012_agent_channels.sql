CREATE TABLE agent_channels (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id       UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    channel_kind   TEXT NOT NULL,
    credentials    JSONB NOT NULL,
    enabled        BOOLEAN NOT NULL DEFAULT true,
    webhook_secret TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(agent_id, channel_kind)
);

CREATE INDEX idx_agent_channels_agent_id ON agent_channels(agent_id);

CREATE TRIGGER trg_agent_channels_updated_at
    BEFORE UPDATE ON agent_channels
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
