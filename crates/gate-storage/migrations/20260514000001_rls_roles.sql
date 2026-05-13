-- ============================================================================
-- RLS Roles & Grants: application role (gate_app) subject to RLS,
-- admin role (gate_admin) bypasses RLS for migrations and ops.
-- ============================================================================

-- Create roles if they don't exist (idempotent)
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'gate_app') THEN
        CREATE ROLE gate_app LOGIN;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'gate_admin') THEN
        CREATE ROLE gate_admin LOGIN BYPASSRLS;
    END IF;
END $$;

-- Grant gate_app access to all tables (RLS will filter)
GRANT USAGE ON SCHEMA public TO gate_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO gate_app;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO gate_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO gate_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE ON SEQUENCES TO gate_app;

-- gate_admin bypasses RLS
GRANT ALL ON ALL TABLES IN SCHEMA public TO gate_admin;
