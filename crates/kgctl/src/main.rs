//! kgctl: Kooix Gate 部署/运维 CLI
//!
//! 当前能力：
//!   kgctl init           生成首次部署所需的全部密钥（master + jwt）
//!   kgctl key master     仅生成 master key (base64 32B)
//!   kgctl key jwt        仅生成 JWT secret (base64 64B)
//!   kgctl env            打印部署必需的环境变量清单（含说明）

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kgctl", version, about = "Kooix Gate 部署/运维工具", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 生成全套首次部署密钥
    Init,
    /// 单独生成某把密钥
    Key {
        #[command(subcommand)]
        which: KeyCmd,
    },
    /// 打印部署所需环境变量清单
    Env,
}

#[derive(Subcommand)]
enum KeyCmd {
    /// 32 字节 master key（AES-256-GCM KEK），用于 envelope encryption
    Master,
    /// 64 字节 JWT HS256 secret
    Jwt,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init => print_init(),
        Cmd::Key { which: KeyCmd::Master } => print_master(),
        Cmd::Key { which: KeyCmd::Jwt } => print_jwt(),
        Cmd::Env => print_env(),
    }
    Ok(())
}

fn print_master() {
    println!("{}", gate_crypto::kms::generate_master_key_b64());
}

fn print_jwt() {
    println!("{}", gate_auth::jwt::generate_secret_b64());
}

fn print_init() {
    let master = gate_crypto::kms::generate_master_key_b64();
    let jwt = gate_auth::jwt::generate_secret_b64();
    println!("# ────────────────────────────────────────────────────────────");
    println!("# Kooix Gate · 首次部署密钥（一次性生成，妥善备份）");
    println!("# ⚠ 这些值丢失即等于所有加密数据全部失效。");
    println!("# 建议：写入密码管理器 / KMS / Vault，至少两份异地备份。");
    println!("# ────────────────────────────────────────────────────────────");
    println!();
    println!("KOOIX_MASTER_KEY={master}");
    println!("KOOIX_JWT_SECRET={jwt}");
    println!();
    println!("# 完整 env 清单见: kgctl env");
}

fn print_env() {
    let entries: &[(&str, &str, &str)] = &[
        ("KOOIX_MASTER_KEY", "必填", "envelope encryption 的 KEK，base64 32B。kgctl key master 生成。"),
        ("KOOIX_JWT_SECRET", "必填", "JWT HS256 secret，base64 64B。kgctl key jwt 生成。"),
        ("KOOIX_DATABASE_URL", "必填", "PostgreSQL，格式 postgres://user:pass@host/db"),
        ("KOOIX_REDIS_URL", "必填", "Redis，格式 redis://host:6379/0"),
        ("KOOIX_LISTEN_ADDR", "可选", "监听地址，默认 0.0.0.0:8000"),
        ("KOOIX_PUBLIC_URL", "必填", "对外可访问的根 URL，用于 OIDC redirect_uri 构造"),
        ("KOOIX_TOKEN_ACCESS_TTL_MIN", "可选", "Access token TTL（分钟），默认 15"),
        ("KOOIX_TOKEN_REFRESH_TTL_DAY", "可选", "Refresh token TTL（天），默认 30"),
        ("RUST_LOG", "可选", "日志级别，建议 info,gate=debug"),
    ];

    println!("Kooix Gate 部署 env 清单：\n");
    println!("{:<32} {:<6} 说明", "变量", "必/可");
    println!("{}", "─".repeat(80));
    for (k, req, desc) in entries {
        println!("{:<32} {:<6} {}", k, req, desc);
    }
}
