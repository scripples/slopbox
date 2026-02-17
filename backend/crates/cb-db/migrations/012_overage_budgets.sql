CREATE TABLE overage_budgets (
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    budget_cents BIGINT NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, period_start)
);

CREATE TRIGGER trg_overage_budgets_updated_at
    BEFORE UPDATE ON overage_budgets
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
