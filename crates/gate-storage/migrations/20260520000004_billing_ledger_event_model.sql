-- Billing ledger event model.
--
-- The original ledger skeleton only represented posted debit rows for request
-- settlement. P1.5 needs explicit event semantics so reconciliation and invoice
-- workflows can distinguish estimated debits, final settles, refunds, manual
-- adjustments, and invoice close markers.

ALTER TABLE billing_ledger_events
    ADD COLUMN IF NOT EXISTS event_type TEXT;

UPDATE billing_ledger_events
SET event_type = 'actual_settle'
WHERE event_type IS NULL;

ALTER TABLE billing_ledger_events
    ALTER COLUMN event_type SET DEFAULT 'actual_settle';

ALTER TABLE billing_ledger_events
    ALTER COLUMN event_type SET NOT NULL;

ALTER TABLE billing_ledger_events
    ADD COLUMN IF NOT EXISTS invoice_month TEXT;

-- Org-level adjustments and invoice close events are not tied to a single
-- project/API key.
ALTER TABLE billing_ledger_events
    ALTER COLUMN project_id DROP NOT NULL;

ALTER TABLE billing_ledger_events
    ALTER COLUMN api_key_id DROP NOT NULL;

ALTER TABLE billing_ledger_events
    DROP CONSTRAINT IF EXISTS billing_ledger_events_event_type_check;

ALTER TABLE billing_ledger_events
    ADD CONSTRAINT billing_ledger_events_event_type_check
    CHECK (
        event_type IN (
            'estimated_debit',
            'actual_settle',
            'refund',
            'manual_adjustment',
            'invoice_close'
        )
    );

ALTER TABLE billing_ledger_events
    DROP CONSTRAINT IF EXISTS billing_ledger_events_direction_check;

ALTER TABLE billing_ledger_events
    ADD CONSTRAINT billing_ledger_events_direction_check
    CHECK (direction IN ('debit', 'credit', 'none'));

ALTER TABLE billing_ledger_events
    DROP CONSTRAINT IF EXISTS billing_ledger_events_invoice_month_check;

ALTER TABLE billing_ledger_events
    ADD CONSTRAINT billing_ledger_events_invoice_month_check
    CHECK (invoice_month IS NULL OR invoice_month ~ '^[0-9]{4}-[0-9]{2}$');

CREATE INDEX IF NOT EXISTS billing_ledger_events_org_month_type_idx
    ON billing_ledger_events (org_id, invoice_month, event_type, occurred_at DESC);

CREATE INDEX IF NOT EXISTS billing_ledger_events_type_time_idx
    ON billing_ledger_events (event_type, occurred_at DESC);
