# ADR-0005: Native Provider Plane —— 双层插件化与三档渠道分级（v0.5.0）

- Status: **Accepted (2026-05-29)** — registry + 通路已落地；kiro / codex PoC 已实现并单测绿（未对真上游验证），windsurf 骨架就位（纯协议逻辑单测绿，gRPC 待接通）
- Deciders: telagod
- Affected: `crates/gate-providers/src/native/`（新增）, `crates/gate-providers/src/router/{builder.rs,helpers.rs,mod.rs}`, `crates/gate-providers/src/lib.rs`
- 关联：收 [ADR-0001](./ADR-0001-providers-as-plugin.md) / [ADR-0004](./ADR-0004-builtin-wrappers-retirement.md) 的"单一接入面"张力；与 [ADR-0003](./ADR-0003-wasm-plugin-abi-v0.md) WASM hook 互补

## Context

[ADR-0001](./ADR-0001-providers-as-plugin.md) 决定全 provider 走 plugin manifest 单一接入面，[ADR-0004](./ADR-0004-builtin-wrappers-retirement.md) 删掉最后 4 个编译期 wrapper，把所有渠道收敛到 `CustomHttpProvider` 一条运行期解释路径。这条路对"标准 HTTP API"渠道是正确且优雅的：新增一个 OpenAI 兼容上游零代码，改 DB 即热加载。

**但 manifest 是声明式配置，它的表达力上限 = 描述一个 HTTP 请求的差异**：URL / method、请求体模板、响应字段 JSON pointer、SSE 帧解析、鉴权策略（Bearer / HMAC / SigV4 / OAuth2）。这个前提对 99% 的上游成立。

存在一类渠道**踩碎了这个前提**——逆向某个产品私有 API 的"重渠道"。对标 `/home/telagod/project/k2i/foxnio` 里的 kiro / windsurf：

- **windsurf**：首次请求时**启动一个本地 Language Server 二进制**，用**手写 gRPC + Protobuf** 调用其 Cascade 接口，管理会话状态机（StartCascade → SendUserCascadeMessage → 轮询 trajectory）、清理服务器路径泄露、两套工具调用模式来回切。（foxnio `providerimpl/windsurf/`，~50 个 Go 文件）
- **kiro**：调 AWS CodeWhisperer / Claude Workbench(CW) 私有接口，做 system prompt 干扰检测后**自动换 profile 重试**，预估 token，buffered stream 里回填 `input_tokens`，规范化 tool name 大小写。（foxnio `providerimpl/kiro/`，~25 个 Go 文件）

这些是**图灵完备的过程逻辑**，不是声明式配置能表达的——你无法用一段 JSONB manifest 写出"fork 一个进程再手写 protobuf 跟它握手"。**这不是 manifest 写得不够好，是范式不匹配**。

foxnio 证明了命令式（code-driven）插件化是可行且优雅的：`Provider` interface 只有 `Descriptor()` + `Handle()` 两个方法（`internal/providerregistry/registry.go`），`Handle()` 是黑盒，任何黑魔法都能装进去；通过 Go `init()` + 包级 `Register()` 静态注册（`internal/providerplugins/all.go`，每个渠道 1 行 import）。

关键洞察：**"用一种插件机制接所有渠道"才是真正的伪命题**。kooix-gate 其实早就有命令式层的地基——`Provider` trait（`lib.rs:225`）就是 `Arc<dyn Provider>` 动态分发，与 foxnio 的 interface 同构。问题只是 `router/builder.rs` 把所有 `provider_type` 强行收口到 `CustomHttpProvider` 一条路，把第二个插槽焊死了。

## Decision

**引入第二层 Native Provider Plane（命令式渠道层），与声明式 manifest 层并存。渠道按接入范式分三档。**

### 三档渠道分级

| 档位 | 渠道类型 | 接入层 | 热加载 | 示例 |
|------|---------|--------|--------|------|
| **轻** | 标准 / OpenAI 兼容 HTTP API | manifest 声明式（`CustomHttpProvider`） | ✅ 改 DB 即生效 | openai, anthropic, gemini, deepseek, 各家云厂商 |
| **中** | 需请求/响应 transform、特殊 header 改写 | manifest + WASM hook（ADR-0003） | ✅ DB + 上传 `.wasm` | 自定义签名、字段重写 |
| **重** | 私有协议 / 起进程 / gRPC / 会话状态机 | **native `Provider` trait（本 ADR）** | ❌ 编译进二进制 | **kiro, windsurf** |

### 接入面判据（默认拒绝 native）

**能用 manifest 表达就必须用 manifest。** native plane 不是回退到 ADR-0004 删掉的 per-provider wrapper，而是为"非 HTTP-API 范式"渠道开的逃生口。新增渠道走 native 的前提是**先证明 manifest（含 WASM hook）表达不了**。这保证 ADR-0001/0004 的"单一接入面"对绝大多数渠道仍然成立，native plane 只承接真正越界的少数。

### 寻址与注册机制

- **寻址**：`channel.provider_type = "native:<name>"`，路由层 strip `native:` 前缀后查注册表。
- **注册表**：进程级 `OnceLock<RwLock<HashMap<String, NativeProviderRegistration>>>`（`native/mod.rs`）。
- **编译期静态注册**：`builtin_registrations()` 里每个重渠道一行 `v.push(<name>::registration())`——对标 foxnio `all.go`。Rust 无 Go `init()` 自动注册，显式列举反而更可控、可测试、不被 dead-code 消除。
- **运行期注册入口**：`register_native_provider()` 给外部 crate / 测试 / 未来动态加载预留。
- **capabilities 自报**：`NativeProviderRegistration.capabilities` 让路由 capability matrix 感知 native 渠道能力，路由层**不写 `if provider == "kiro"`**——对标 foxnio `Descriptor.Capabilities`。
- **secret 透传**：路由复用既有 `resolve_secrets_for_channel`，把解密后的多 slot secrets 经 `NativeBuildContext` 交给 factory；native 实现自取（kiro 取 token，windsurf 解析 ProfileArn）。

### 收口动作

| # | 动作 | 位点 | 状态 |
|---|------|------|------|
| 1 | 新增 `native/mod.rs`：registry + `NativeBuildContext` + factory + `register/build/capabilities/names` 公共 API | `crates/gate-providers/src/native/mod.rs` | ✅ |
| 2 | `native/echo.rs`：通路验证 fixture（仅测试编译），证明 trait 这层活过来 | `crates/gate-providers/src/native/echo.rs` | ✅ |
| 3 | `builder.rs`：`build_provider_with_secrets` 开头加 `native:` 前缀分支早返回 | `router/builder.rs` | ✅ |
| 4 | `helpers.rs`：`is_native_provider` 谓词 + `channel_capabilities` native 分支 | `router/helpers.rs` | ✅ |
| 5 | `mod.rs`：chat 路由 `needs_secret_slots` 让 native 也走 `with_secrets` 路径 | `router/mod.rs:1710` | ✅ |
| 6 | `lib.rs`：`pub mod native` + re-export | `lib.rs` | ✅ |
| 7 | kiro 移植为首个 native 重渠道（CW 协议、auth、req/resp 转换） | `native/kiro.rs` | 🚧 PoC（6 单测绿，未对真上游验证） |
| 8 | codex 移植（ChatGPT backend Responses API、SSE、claude→gpt model+effort 联动映射） | `native/codex.rs` | 🚧 PoC（6 单测绿，未对真上游验证） |
| 9 | windsurf 骨架（fork 本地 LS + HTTP/2 gRPC + Cascade 状态机） | `native/windsurf.rs` | 🚧 骨架（7 单测绿：varint/proto/model/tool 纯逻辑；gRPC 未接通，factory fail-loud） |
| 10 | `/<渠道名>` 显式寻址（少一层 group 路由，用户直接指定渠道） | server 路由层 | ⬜ 后续 |
| 11 | native 渠道 secret gate（按需校验 token 存在，类比 plugin `has_available_plugin_secret`） | `router/mod.rs` | ⬜ 后续 |
| 12 | admin / UI 暴露 `native_provider_names()`，渠道创建可选 native 类型 | server + web | ⬜ 后续 |

### 设计约束

1. **重渠道无热加载银弹**：native 重渠道要起进程 / 用原生网络栈，WASM 沙箱做不到 fork/exec/任意 socket，**注定编译进二进制**，享受不到 manifest 的"改 DB 即生效"。foxnio 同此约束（static import + 重编译）。这是物理约束，不是设计缺陷——CHANGELOG 需明示。

2. **既有 plugin 路径零改动**：native 接入是纯增量旁路（builder 前置 if、chat 分流 `||`、capabilities 加分支），声明式层逻辑不变，回归面极小。

3. **fail-loud**：`provider_type='native:<name>'` 但 name 未注册时，`build_native_provider` 给出明确 error（列出已注册 native providers），不静默回退到 HTTP。

4. **capability 精确化**：在 native 自报 capabilities 之前，未注册 native 类型会落到 `provider_capabilities` 的 `_` 默认（`openai_compatible_core`），导致路由误判可路由。注册后以 registration 声明为准。

### 参考但不照搬：cc-switch

[cc-switch](https://github.com/farion1231/cc-switch)（Tauri + Rust 桌面配置切换器）值得借鉴的只有两点：

- **ProviderAdapter 适配器抽象** —— 对应我们的 `Provider` trait / native registration，思路一致。
- **渠道熔断思路** —— 对应已有的 `ChannelMetrics` 滑窗成功率 auto-disable（`router/metrics.rs`）。

**明确不学**其"以 Anthropic 为中心的点对点直转 + 动态 JSON 归一化"：那是桌面单连接场景的便利做法，对高性能多渠道网关是反面教材（每加一个目标格式就是 N×M 点对点转换，且动态 JSON 丢类型）。kooix-gate 坚持"统一 OpenAI 兼容内部协议 + 各渠道在自己边界吸收差异"的星形归一化（`types.rs` 统一类型），native 渠道在 `Handle` 等价物（`chat`/`chat_stream`）里把上游协议收口到统一 `ChatResponse`。

## Consequences

### Positive

- kiro / windsurf 这类重渠道终于有了接入路径，不再被 manifest 范式拒之门外。
- 命令式层与声明式层并存，各司其职；ADR-0001/0004 单一接入面战略对轻/中渠道依然成立。
- 复用既有 `Provider` trait + 路由 + secret 解析 + capability matrix + 熔断，native 实现只需写"上游协议适配"这一块黑盒。
- `native:<name>` 寻址为 `/<渠道名>` 显式调用、少一层路由打好基础。

### Negative / Risks

- **重渠道需编译进二进制**：加一个 native 渠道要改 `builtin_registrations` + 重编译 + 重部署，无热加载。
- **黑盒自由度的双刃**：`Provider` trait 不约束实现规范，native 渠道可做任意 I/O（起进程、连 socket），需配套出站沙箱 / 资源限制审计（windsurf 起 LS 进程是高权限操作）。
- **capability 默认值陷阱**：见设计约束 4，未注册 native 类型的 channel 会被误判可路由，必须确保 `provider_type` 与注册名一致。

### Verification

- [x] `native/mod.rs` 4 单测绿：`native_name_strips_prefix` / `echo_is_registered_and_advertises_chat` / `echo_roundtrips_chat` / `unknown_native_provider_is_fail_loud`
- [x] `cargo test -p gate-providers` 全量绿（lib 165 + 6 集成套件），native 平面共 23 单测（mod 7 / kiro 6 / codex 6 / windsurf 7 / echo 嵌入），既有测试不回归
- [x] kiro native PoC 单测：`build_body` conversationState 形状、EventStream 帧解析、model 规范化
- [x] codex native PoC 单测：`build_body` Responses 形状、SSE 文本/usage 解析、claude→gpt model+effort 映射
- [x] windsurf 骨架单测：protobuf varint/tag/gRPC 帧、model 归一化、回退式 tool_call 提取、factory fail-loud
- [ ] kiro / codex native PoC：对真上游发一条 chat 拿到回复（非流式 + 流式 SSE）
- [ ] windsurf：fork 本地 LS + HTTP/2 gRPC 接通，Cascade 状态机跑通一轮对话
- [ ] 路由级集成测试：`native:echo`/`native:kiro`/`native:codex` channel 能被 `route_chat` 选中并构造 provider
- [ ] native 渠道 secret gate 落地（token 缺失时 skip + 明确诊断）
- [ ] CHANGELOG 标注"native 重渠道需编译进二进制、无热加载"

## References

- [ADR-0001 Provider 全插件化迁移](./ADR-0001-providers-as-plugin.md)
- [ADR-0004 4 大编译期 wrapper 退役](./ADR-0004-builtin-wrappers-retirement.md)
- [ADR-0003 WASM Plugin ABI v0](./ADR-0003-wasm-plugin-abi-v0.md)（中档渠道 transform 层）
- foxnio 命令式插件化参考：`k2i/foxnio/backend/internal/providerregistry/registry.go`、`providerimpl/{kiro,windsurf}/`、`providerplugins/all.go`
- cc-switch：https://github.com/farion1231/cc-switch （仅取 ProviderAdapter 抽象 + 熔断思路）
