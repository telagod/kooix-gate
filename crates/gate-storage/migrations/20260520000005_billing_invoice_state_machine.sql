-- Billing invoice state machine.
--
-- P1.5 closes the gap between monthly bill aggregation and an auditable invoice
-- lifecycle. The aggregate remains rebuildable from ledger, while this table
-- records the operational state: draft -> closed -> exported -> paid/waived.

CREATE TABLE IF NOT EXISTS billing_invoices (
    org_id              UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    month               TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'draft'
                        CHECK (status IN ('draft', 'closed', 'exported', 'paid', 'waived')),
    total_cost_usd      NUMERIC NOT NULL DEFAULT 0,
    total_cost_micros   BIGINT NOT NULL DEFAULT 0 CHECK (total_cost_micros >= 0),
    export_digest       TEXT,
    closed_at           TIMESTAMPTZ,
    exported_at         TIMESTAMPTZ,
    paid_at             TIMESTAMPTZ,
    waived_at           TIMESTAMPTZ,
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, month),
    CHECK (month ~ '^[0-9]{4}-[0-9]{2}$'),
    CHECK (export_digest IS NULL OR export_digest ~ '^sha256:[0-9a-f]{64}$'),
    CHECK (status <> 'closed' OR closed_at IS NOT NULL),
    CHECK (status <> 'exported' OR (exported_at IS NOT NULL AND export_digest IS NOT NULL)),
    CHECK (status <> 'paid' OR paid_at IS NOT NULL),
    CHECK (status <> 'waived' OR waived_at IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS billing_invoices_status_idx
    ON billing_invoices (status, updated_at DESC);

CREATE TRIGGER billing_invoices_updated_at BEFORE UPDATE ON billing_invoices
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
