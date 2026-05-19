//! `kgctl usage-storage` — usage/read-model storage dry-run planner.
//!
//! 不直接执行 DDL；只输出普通 PostgreSQL 分区/retention 或 Timescale hypertable
//! 方案，供 DBA 审阅后手工执行或放入受控 migration。

use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub enum UsageStoragePlanKind {
    Partition,
    Timescale,
}

pub fn plan(kind: UsageStoragePlanKind, months_ahead: u32, retention_months: u32) -> Result<()> {
    let months_ahead = months_ahead.clamp(1, 24);
    let retention_months = retention_months.clamp(1, 120);
    match kind {
        UsageStoragePlanKind::Partition => print_partition_plan(months_ahead, retention_months),
        UsageStoragePlanKind::Timescale => print_timescale_plan(retention_months),
    }
    Ok(())
}

fn print_partition_plan(months_ahead: u32, retention_months: u32) {
    println!("-- Kooix Gate usage storage plan: PostgreSQL native monthly partitions");
    println!("-- dry-run only; review before applying in production");
    println!(
        "-- future partitions: {months_ahead} month(s); retention: {retention_months} month(s)"
    );
    println!();
    println!("BEGIN;");
    println!(
        "-- 1) Keep existing tables online and create partitioned successors in a controlled migration."
    );
    println!(
        "--    Example shown for request_events; repeat same pattern for usage_records if legacy hot reads remain."
    );
    println!(
        "CREATE TABLE IF NOT EXISTS request_events_partitioned (LIKE request_events INCLUDING ALL) PARTITION BY RANGE (ts);"
    );
    println!(
        "CREATE TABLE IF NOT EXISTS usage_records_partitioned (LIKE usage_records INCLUDING ALL) PARTITION BY RANGE (ts);"
    );
    println!();
    println!("-- 2) Create this month + future monthly partitions.");
    println!("DO $$");
    println!("DECLARE");
    println!("  base_month date := date_trunc('month', now())::date;");
    println!("  i int;");
    println!("  part_start date;");
    println!("  part_end date;");
    println!("  suffix text;");
    println!("BEGIN");
    println!("  FOR i IN 0..{months_ahead} LOOP");
    println!("    part_start := (base_month + (i || ' months')::interval)::date;");
    println!("    part_end := (part_start + interval '1 month')::date;");
    println!("    suffix := to_char(part_start, 'YYYY_MM');");
    println!(
        "    EXECUTE format('CREATE TABLE IF NOT EXISTS request_events_%s PARTITION OF request_events_partitioned FOR VALUES FROM (%L) TO (%L)', suffix, part_start, part_end);"
    );
    println!(
        "    EXECUTE format('CREATE TABLE IF NOT EXISTS usage_records_%s PARTITION OF usage_records_partitioned FOR VALUES FROM (%L) TO (%L)', suffix, part_start, part_end);"
    );
    println!("  END LOOP;");
    println!("END $$;");
    println!();
    println!(
        "-- 3) Retention dry-run query: inspect partitions older than retention window before dropping."
    );
    println!("SELECT inhrelid::regclass AS candidate_partition");
    println!("FROM pg_inherits");
    println!(
        "WHERE inhparent IN ('request_events_partitioned'::regclass, 'usage_records_partitioned'::regclass)"
    );
    println!(
        "  AND regexp_replace(inhrelid::regclass::text, '^.*_(\\\\d{{4}}_\\\\d{{2}})$', '\\\\1') <"
    );
    println!(
        "      to_char(date_trunc('month', now()) - interval '{retention_months} months', 'YYYY_MM');"
    );
    println!("COMMIT;");
}

fn print_timescale_plan(retention_months: u32) {
    println!("-- Kooix Gate usage storage plan: TimescaleDB optional profile");
    println!("-- dry-run only; requires extension to be installed by DBA");
    println!("-- retention: {retention_months} month(s)");
    println!();
    println!("BEGIN;");
    println!("CREATE EXTENSION IF NOT EXISTS timescaledb;");
    println!("SELECT create_hypertable('request_events', by_range('ts'), if_not_exists => TRUE);");
    println!("SELECT create_hypertable('usage_records', by_range('ts'), if_not_exists => TRUE);");
    println!(
        "ALTER TABLE request_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'org_id,project_id,model_actual');"
    );
    println!(
        "ALTER TABLE usage_records SET (timescaledb.compress, timescaledb.compress_segmentby = 'org_id,project_id,model_actual');"
    );
    println!(
        "SELECT add_compression_policy('request_events', INTERVAL '7 days', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_compression_policy('usage_records', INTERVAL '7 days', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_retention_policy('request_events', INTERVAL '{retention_months} months', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_retention_policy('usage_records', INTERVAL '{retention_months} months', if_not_exists => TRUE);"
    );
    println!("COMMIT;");
}
