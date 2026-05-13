//! kgctl: Kooix Gate 部署/运维 CLI
//!
//! 子命令：
//!   kgctl init               生成首次部署所需的全部密钥（master + jwt）
//!   kgctl key master         仅生成 master key (base64 32B)
//!   kgctl key jwt            仅生成 JWT secret (base64 64B)
//!   kgctl env                打印部署必需的环境变量清单（含说明）
//!   kgctl migrate            连 DB 跑 migrations（--dry-run 仅列出 pending）
//!   kgctl admin create       创建 platform super_admin 账号
//!   kgctl doctor             一键体检：env / DB / Redis 可达性
//!   kgctl seed-pricing       写入主流模型默认定价（幂等）

use clap::{Parser, Subcommand};

mod admin;
mod doctor;
mod migrate;
mod pricing;

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
    /// 跑 PostgreSQL 数据库迁移
    Migrate {
        /// 仅列出待执行的 migration 不实际执行
        #[arg(long)]
        dry_run: bool,
    },
    /// 平台级账号管理
    Admin {
        #[command(subcommand)]
        sub: AdminCmd,
    },
    /// 部署体检：env 完整性 + DB / Redis 可达性
    Doctor,
    /// 写入主流模型的默认计费定价（幂等）
    SeedPricing,
}

#[derive(Subcommand)]
enum KeyCmd {
    /// 32 字节 master key（AES-256-GCM KEK），用于 envelope encryption
    Master,
    /// 64 字节 JWT HS256 secret
    Jwt,
}

#[derive(Subcommand)]
enum AdminCmd {
    /// 创建 super_admin 账号（首次部署后用一次）
    Create {
        /// 登录邮箱（CITEXT 唯一）
        #[arg(long)]
        email: String,
        /// 初始密码；不传则自动生成 24-byte base64url 随机密码并打印一次
        #[arg(long)]
        password: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let result: anyhow::Result<()> = match cli.cmd {
        Cmd::Init => {
            print_init();
            Ok(())
        }
        Cmd::Key {
            which: KeyCmd::Master,
        } => {
            print_master();
            Ok(())
        }
        Cmd::Key { which: KeyCmd::Jwt } => {
            print_jwt();
            Ok(())
        }
        Cmd::Env => {
            print_env();
            Ok(())
        }
        Cmd::Migrate { dry_run } => run_async(migrate::run(dry_run)),
        Cmd::Admin {
            sub: AdminCmd::Create { email, password },
        } => run_async(admin::create(email, password)),
        Cmd::Doctor => run_async(doctor::run()),
        Cmd::SeedPricing => run_async(pricing::seed()),
    };

    if let Err(e) = result {
        // 红色错误 + 非零退出
        eprintln!("\x1b[31merror:\x1b[0m {e:#}");
        std::process::exit(1);
    }
}

fn run_async<F>(fut: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(fut)
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
        (
            "KOOIX_MASTER_KEY",
            "必填",
            "envelope encryption 的 KEK，base64 32B。kgctl key master 生成。",
        ),
        (
            "KOOIX_JWT_SECRET",
            "必填",
            "JWT HS256 secret，base64 ≥32B。kgctl key jwt 生成。",
        ),
        (
            "KOOIX_DATABASE_URL",
            "必填",
            "PostgreSQL，格式 postgres://user:pass@host/db",
        ),
        ("KOOIX_REDIS_URL", "必填", "Redis，格式 redis://host:6379/0"),
        ("KOOIX_LISTEN_ADDR", "可选", "监听地址，默认 0.0.0.0:8000"),
        (
            "KOOIX_PUBLIC_URL",
            "必填",
            "对外可访问的根 URL，用于 OIDC redirect_uri 构造",
        ),
        (
            "KOOIX_TOKEN_ACCESS_TTL_MIN",
            "可选",
            "Access token TTL（分钟），默认 15",
        ),
        (
            "KOOIX_TOKEN_REFRESH_TTL_DAY",
            "可选",
            "Refresh token TTL（天），默认 30",
        ),
        (
            "KOOIX_OIDC_DEFAULT_REDIRECT",
            "可选",
            "SSO 登录成功后默认跳转 URL（未带 redirect 参数时使用）",
        ),
        ("RUST_LOG", "可选", "日志级别，建议 info,gate=debug"),
    ];

    println!("Kooix Gate 部署 env 清单：\n");
    println!("{:<32} {:<6} 说明", "变量", "必/可");
    println!("{}", "─".repeat(80));
    for (k, req, desc) in entries {
        println!("{:<32} {:<6} {}", k, req, desc);
    }
}
