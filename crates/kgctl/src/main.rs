//! kgctl: Kooix Gate 部署/运维 CLI
//!
//! 子命令：
//!   kgctl setup              交互式初次启动引导（推荐首次部署使用）
//!   kgctl init               生成首次部署所需的全部密钥（master + jwt）
//!   kgctl key master         仅生成 master key (base64 32B)
//!   kgctl key jwt            仅生成 JWT secret (base64 64B)
//!   kgctl env                打印部署必需的环境变量清单（含说明）
//!   kgctl migrate            连 DB 跑 migrations（--dry-run 仅列出 pending）
//!   kgctl admin create       创建 platform super_admin 账号
//!   kgctl doctor             一键体检：env / DB / Redis 可达性（支持 --json）
//!   kgctl smoke              对已运行 gate-server 做最小 HTTP 冒烟
//!   kgctl seed-pricing       写入主流模型默认定价（幂等）
//!   kgctl usage-storage plan 输出 usage 热表分区 / Timescale dry-run DDL

use clap::{Args, Parser, Subcommand};

mod admin;
mod doctor;
mod migrate;
mod plugin;
mod pricing;
mod setup;
mod smoke;
mod usage_storage;

#[derive(Parser)]
#[command(name = "kgctl", version, about = "Kooix Gate 部署/运维工具", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 交互式初次启动引导（推荐首次部署使用）
    Setup,
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
    Doctor {
        /// 输出机器可读 JSON，适合 CI / deploy pipeline 消费
        #[arg(long)]
        json: bool,
    },
    /// 发布后 HTTP 冒烟：登录、建 channel/API key、发 chat、查 usage
    Smoke {
        /// gate-server 根 URL；默认读 KOOIX_PUBLIC_URL
        #[arg(long, env = "KOOIX_PUBLIC_URL")]
        base_url: String,
        /// 登录邮箱；默认读 KOOIX_SMOKE_EMAIL
        #[arg(long, env = "KOOIX_SMOKE_EMAIL")]
        email: String,
        /// 登录密码；默认读 KOOIX_SMOKE_PASSWORD
        #[arg(long, env = "KOOIX_SMOKE_PASSWORD")]
        password: String,
        /// 可选 OpenAI-compatible 上游 base URL；提供后 smoke 会创建 channel/group/default route
        #[arg(long, env = "KOOIX_SMOKE_UPSTREAM_BASE_URL")]
        upstream_base_url: Option<String>,
        /// 上游 API key；创建 channel key 时使用，不会打印完整值
        #[arg(
            long,
            env = "KOOIX_SMOKE_UPSTREAM_API_KEY",
            default_value = "sk-kgctl-smoke"
        )]
        upstream_api_key: String,
        /// smoke 使用的模型名
        #[arg(long, env = "KOOIX_SMOKE_MODEL", default_value = "gpt-4o-mini")]
        model: String,
    },
    /// 写入主流模型的默认计费定价（幂等，legacy model_pricing 表）
    SeedPricing,
    /// 定价规则管理（pricing_rules 表）
    Pricing {
        #[command(subcommand)]
        sub: PricingCmd,
    },
    /// Usage/request_events 存储规划（仅 dry-run 输出，不执行 DDL）
    UsageStorage {
        #[command(subcommand)]
        sub: UsageStorageCmd,
    },
    /// HTTP Plugin manifest 工具
    Plugin {
        #[command(subcommand)]
        sub: Box<PluginCmd>,
    },
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

#[derive(Subcommand)]
enum PricingCmd {
    /// 列出定价规则
    List {
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        channel_id: Option<String>,
    },
    /// 创建/更新定价规则
    Set {
        #[arg(long)]
        model: String,
        #[arg(long)]
        dimension: String,
        #[arg(long)]
        unit: String,
        #[arg(long)]
        rate: f64,
        #[arg(long)]
        channel_id: Option<String>,
        #[arg(long, default_value = "0")]
        priority: i32,
        #[arg(long)]
        description: Option<String>,
    },
    /// 删除定价规则
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum UsageStorageCmd {
    /// 输出 PostgreSQL 分区或 Timescale 可选方案 SQL（dry-run）
    Plan {
        /// 输出 Timescale hypertable/compression/retention 方案
        #[arg(long, conflicts_with = "partition")]
        timescale: bool,
        /// 输出普通 PostgreSQL 月分区方案（默认）
        #[arg(long)]
        partition: bool,
        /// 预创建未来几个月分区（普通 PostgreSQL 方案）
        #[arg(long, default_value = "3")]
        months_ahead: u32,
        /// retention 窗口（月）
        #[arg(long, default_value = "18")]
        retention_months: u32,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// 输出 HTTP Plugin manifest v1 JSON Schema
    Schema,
    /// 校验 manifest JSON（默认从 stdin 读取，path 可传文件或 -）
    Lint {
        /// manifest 文件路径；不传或传 - 时读 stdin
        path: Option<String>,
        /// 用于 preset 展开与绝对路径校验的上游 base URL
        #[arg(long, default_value = "https://example.com")]
        base_url: String,
    },
    /// 回放一段 raw SSE，输出归一后的 OpenAI-compatible chunks
    Replay {
        /// manifest 文件路径；不传或传 - 时读 stdin
        manifest: Option<String>,
        /// raw SSE 文件路径
        #[arg(long)]
        sse: String,
        /// 用于 preset 展开与绝对路径校验的上游 base URL
        #[arg(long, default_value = "https://example.com")]
        base_url: String,
        /// replay fallback model
        #[arg(long, default_value = "replay-model")]
        model: String,
    },
    /// 对上游发一次 non-stream chat，验证 manifest request/response mapping
    Test {
        /// manifest 文件路径；不传或传 - 时读 stdin
        manifest: Option<String>,
        /// 上游 base URL
        #[arg(long, default_value = "https://example.com")]
        base_url: String,
        /// 测试用 API key；默认可读 KOOIX_PLUGIN_TEST_API_KEY
        #[arg(
            long,
            env = "KOOIX_PLUGIN_TEST_API_KEY",
            default_value = "sk-kgctl-plugin-test"
        )]
        api_key: String,
        /// 测试模型
        #[arg(long, default_value = "replay-model")]
        model: String,
        /// 测试 prompt
        #[arg(long, default_value = "Hi")]
        prompt: String,
        /// max_tokens
        #[arg(long, default_value = "1")]
        max_tokens: u32,
        /// 请求超时
        #[arg(long, default_value = "15000")]
        timeout_ms: u64,
    },
    /// 导出 manifest golden fixture（可包含 response sample 与 raw SSE 期望 chunks）
    Export {
        /// manifest 文件路径；不传或传 - 时读 stdin
        manifest: Option<String>,
        /// raw SSE 文件路径；提供后会生成 expected_chunks
        #[arg(long)]
        sse: Option<String>,
        /// non-stream response sample JSON 文件路径
        #[arg(long)]
        response_sample: Option<String>,
        /// 输出 fixture 文件；不传或传 - 时写 stdout
        #[arg(short, long)]
        output: Option<String>,
        /// 用于 preset 展开与绝对路径校验的上游 base URL
        #[arg(long, default_value = "https://example.com")]
        base_url: String,
        /// replay fallback model
        #[arg(long, default_value = "replay-model")]
        model: String,
    },
    /// 导入/校验 manifest golden fixture，并可导出其中的 manifest
    Import {
        /// fixture 文件路径；不传或传 - 时读 stdin
        fixture: Option<String>,
        /// 回放 raw SSE 并与 expected_chunks 做 golden 比对
        #[arg(long)]
        verify: bool,
        /// 输出 manifest 文件；传 - 写 stdout
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Manifest registry / package 工具
    Registry {
        #[command(subcommand)]
        sub: Box<PluginRegistryCmd>,
    },
}

#[derive(Subcommand)]
enum PluginRegistryCmd {
    /// 列出 registry entries，默认读取 examples/manifest-registry
    List {
        /// registry 根目录，需包含 registry.json
        #[arg(long)]
        root: Option<String>,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 把 manifest + README/security/fixtures 打包成可导入 package JSON
    Package(Box<PluginRegistryPackageArgs>),
    /// 导入 package 到私有 registry
    Import {
        /// package JSON 路径
        package: String,
        /// registry 根目录
        #[arg(long, default_value = "examples/manifest-registry")]
        root: String,
        /// 私有 namespace
        #[arg(long, default_value = "local")]
        namespace: String,
        /// 回放 package fixtures
        #[arg(long)]
        verify: bool,
        /// 允许导入 unsigned package
        #[arg(long)]
        allow_unsigned: bool,
        /// 用于 manifest lint 的 base URL
        #[arg(long, default_value = "https://example.com")]
        base_url: String,
    },
    /// 导出 registry index，可选择是否包含 private entries
    Export {
        /// registry 根目录
        #[arg(long, default_value = "examples/manifest-registry")]
        root: String,
        /// 输出 registry JSON；不传或 - 写 stdout
        #[arg(short, long)]
        output: Option<String>,
        /// 包含 private entries
        #[arg(long)]
        include_private: bool,
    },
}

#[derive(Args)]
struct PluginRegistryPackageArgs {
    /// registry id，使用小写 [a-z0-9_-]
    #[arg(long)]
    id: String,
    /// 展示名
    #[arg(long)]
    name: String,
    /// package version，MAJOR.MINOR.PATCH
    #[arg(long)]
    version: String,
    /// author / maintainer
    #[arg(long)]
    author: String,
    /// official / community / private
    #[arg(long, default_value = "private")]
    source: String,
    /// manifest.json 路径
    #[arg(long)]
    manifest: String,
    /// 输出 package JSON 路径
    #[arg(short, long)]
    output: String,
    /// README.md 路径；不传则生成占位说明
    #[arg(long)]
    readme: Option<String>,
    /// security.md 路径；不传则生成占位风险说明
    #[arg(long)]
    security: Option<String>,
    /// 可重复传入 fixture JSON
    #[arg(long = "fixture")]
    fixtures: Vec<String>,
    /// 兼容的最小 Kooix Gate 版本
    #[arg(long, default_value = "0.2.0")]
    min_gate_version: String,
    /// 兼容的最大 Kooix Gate 版本
    #[arg(long)]
    max_gate_version: Option<String>,
    /// unsigned / cosign / minisign / sigstore_bundle
    #[arg(long, default_value = "unsigned")]
    signature_kind: String,
    /// 签名值或签名 bundle 引用
    #[arg(long)]
    signature: Option<String>,
    /// 项目主页
    #[arg(long)]
    homepage: Option<String>,
    /// 标签，可重复
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// 描述
    #[arg(long)]
    description: Option<String>,
    /// 用于 preset 展开与绝对路径校验的 base URL
    #[arg(long, default_value = "https://example.com")]
    base_url: String,
}

fn main() {
    let cli = Cli::parse();
    let result: anyhow::Result<()> = match cli.cmd {
        Cmd::Setup => run_async(setup::run()),
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
        Cmd::Doctor { json } => run_async(doctor::run(doctor::DoctorOutput::from_json_flag(json))),
        Cmd::Smoke {
            base_url,
            email,
            password,
            upstream_base_url,
            upstream_api_key,
            model,
        } => run_async(smoke::run(smoke::SmokeOpts {
            base_url,
            email,
            password,
            upstream_base_url,
            upstream_api_key,
            model,
        })),
        Cmd::SeedPricing => run_async(pricing::seed()),
        Cmd::Pricing {
            sub: PricingCmd::List { model, channel_id },
        } => run_async(pricing::list(model, channel_id)),
        Cmd::Pricing {
            sub:
                PricingCmd::Set {
                    model,
                    dimension,
                    unit,
                    rate,
                    channel_id,
                    priority,
                    description,
                },
        } => run_async(pricing::set(
            model,
            dimension,
            unit,
            rate,
            channel_id,
            priority,
            description,
        )),
        Cmd::Pricing {
            sub: PricingCmd::Delete { id },
        } => run_async(pricing::delete(id)),
        Cmd::UsageStorage {
            sub:
                UsageStorageCmd::Plan {
                    timescale,
                    partition: _,
                    months_ahead,
                    retention_months,
                },
        } => {
            let kind = if timescale {
                usage_storage::UsageStoragePlanKind::Timescale
            } else {
                usage_storage::UsageStoragePlanKind::Partition
            };
            usage_storage::plan(kind, months_ahead, retention_months)
        }
        Cmd::Plugin { sub } => match *sub {
            PluginCmd::Schema => plugin::schema(),
            PluginCmd::Lint { path, base_url } => plugin::lint(path, base_url),
            PluginCmd::Replay {
                manifest,
                sse,
                base_url,
                model,
            } => plugin::replay(manifest, sse, base_url, model),
            PluginCmd::Test {
                manifest,
                base_url,
                api_key,
                model,
                prompt,
                max_tokens,
                timeout_ms,
            } => run_async(plugin::test_connection(
                manifest, base_url, api_key, model, prompt, max_tokens, timeout_ms,
            )),
            PluginCmd::Export {
                manifest,
                sse,
                response_sample,
                output,
                base_url,
                model,
            } => plugin::export_fixture(manifest, sse, response_sample, output, base_url, model),
            PluginCmd::Import {
                fixture,
                verify,
                output,
            } => plugin::import_fixture(fixture, verify, output),
            PluginCmd::Registry { sub } => match *sub {
                PluginRegistryCmd::List { root, json } => plugin::registry_list(root, json),
                PluginRegistryCmd::Package(args) => {
                    plugin::registry_package(plugin::RegistryPackageInput {
                        id: args.id,
                        name: args.name,
                        version: args.version,
                        author: args.author,
                        source: args.source,
                        manifest_path: args.manifest,
                        output: args.output,
                        readme_path: args.readme,
                        security_path: args.security,
                        fixture_paths: args.fixtures,
                        min_gate_version: args.min_gate_version,
                        max_gate_version: args.max_gate_version,
                        signature_kind: args.signature_kind,
                        signature_value: args.signature,
                        homepage: args.homepage,
                        tags: args.tags,
                        description: args.description,
                        base_url: args.base_url,
                    })
                }
                PluginRegistryCmd::Import {
                    package,
                    root,
                    namespace,
                    verify,
                    allow_unsigned,
                    base_url,
                } => plugin::registry_import(plugin::RegistryImportInput {
                    package_path: package,
                    registry_root: root,
                    private_namespace: namespace,
                    verify,
                    allow_unsigned,
                    base_url,
                }),
                PluginRegistryCmd::Export {
                    root,
                    output,
                    include_private,
                } => plugin::registry_export(plugin::RegistryExportInput {
                    registry_root: root,
                    output,
                    include_private,
                }),
            },
        },
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
            "JWT HS256 primary signing secret，base64 ≥32B。kgctl key jwt 生成。",
        ),
        (
            "KOOIX_JWT_PREVIOUS_SECRETS",
            "可选",
            "JWT rotation 验签窗口旧 secret，逗号分隔 base64 ≥32B；只验签不签发。",
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
        println!("{k:<32} {req:<6} {desc}");
    }
}
