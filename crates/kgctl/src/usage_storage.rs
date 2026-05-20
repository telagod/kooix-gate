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
    println!("-- 1) Ensure the partitioned request-log projection and future partitions.");
    println!("SELECT kooix_ensure_request_log_partitions({months_ahead});");
    println!();
    println!("-- 2) Equivalent manual DDL shape if you need DBA-reviewed SQL.");
    println!(
        "CREATE TABLE IF NOT EXISTS request_log_events (LIKE request_events INCLUDING DEFAULTS INCLUDING COMMENTS) PARTITION BY RANGE (ts);"
    );
    println!("DO $$");
    println!("DECLARE");
    println!("  base_month timestamptz := date_trunc('month', now());");
    println!("  i int;");
    println!("  part_start timestamptz;");
    println!("  part_end timestamptz;");
    println!("  suffix text;");
    println!("BEGIN");
    println!("  FOR i IN 0..{months_ahead} LOOP");
    println!("    part_start := (base_month + (i || ' months')::interval)::date;");
    println!("    part_end := (part_start + interval '1 month')::date;");
    println!("    suffix := to_char(part_start, 'YYYY_MM');");
    println!(
        "    EXECUTE format('CREATE TABLE IF NOT EXISTS request_log_events_%s PARTITION OF request_log_events FOR VALUES FROM (%L) TO (%L)', suffix, part_start, part_end);"
    );
    println!("  END LOOP;");
    println!("END $$;");
    println!();
    println!("-- 3) Retention dry-run query: inspect partitions older than retention window.");
    println!("SELECT * FROM kooix_prune_request_log_partitions({retention_months}, TRUE);");
    println!("-- Apply after review:");
    println!("-- SELECT * FROM kooix_prune_request_log_partitions({retention_months}, FALSE);");
    println!(
        "-- SELECT kooix_prune_request_log_details({});",
        retention_months * 31
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
    println!(
        "SELECT create_hypertable('request_log_events', by_range('ts'), if_not_exists => TRUE);"
    );
    println!("SELECT create_hypertable('usage_records', by_range('ts'), if_not_exists => TRUE);");
    println!(
        "ALTER TABLE request_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'org_id,project_id,model_actual');"
    );
    println!(
        "ALTER TABLE request_log_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'org_id,project_id,model_actual');"
    );
    println!(
        "ALTER TABLE usage_records SET (timescaledb.compress, timescaledb.compress_segmentby = 'org_id,project_id,model_actual');"
    );
    println!(
        "SELECT add_compression_policy('request_events', INTERVAL '7 days', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_compression_policy('request_log_events', INTERVAL '7 days', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_compression_policy('usage_records', INTERVAL '7 days', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_retention_policy('request_events', INTERVAL '{retention_months} months', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_retention_policy('request_log_events', INTERVAL '{retention_months} months', if_not_exists => TRUE);"
    );
    println!(
        "SELECT add_retention_policy('usage_records', INTERVAL '{retention_months} months', if_not_exists => TRUE);"
    );
    println!("COMMIT;");
}
