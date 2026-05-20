# Kooix Gate — Web Console

SvelteKit + TypeScript + Tailwind CSS 前端控制台，对接 `gate-server` HTTP API。

## 依赖安装

```bash
npm install
# 或
pnpm install
```

## 本地开发

```bash
# 复制环境变量
cp .env.example .env.local

# 启动前端开发服务器（默认 http://localhost:5173）
npm run dev
# 或
pnpm dev
```

## 后端配合

前端通过 `VITE_API_BASE_URL`（默认 `http://localhost:3000`）对接后端。

启动本地 gate-server（内存模式，无需 PG）：

```bash
KOOIX_DEV_INMEMORY=1 cargo run -p gate-server
```

CORS 在 gate-server 已设置为 `CorsLayer::permissive()`，本地无需额外配置。

## 构建

```bash
npm run build
# 或
pnpm build
```

构建产物在 `.svelte-kit/output/`（adapter-auto 输出）。

## 设计模板

前端视觉语言集中在 `src/lib/design/` 与 `src/lib/components/templates/`：

- `src/lib/design/classes.ts`：统一 text / surface / button / card / control / badge / alert class 工厂。
- `src/lib/components/templates/`：`PageShell`、`AuthFrame`、`SectionCard`、`StatePanel`、`ModalFrame`、`DataToolbar`、`FilterPanel`、`DataTable` 页面模板。
- `src/lib/components/ui/index.ts`：统一导出基础 UI 组件。

新页面优先使用模板，避免在 route 内复制基础按钮、输入框、卡片长 class。详细规范见 `src/lib/design/README.md`。
当前前端质量门禁：`npm run check` 必须保持 `0 errors / 0 warnings`；`npm run build` 不应输出大 chunk、Rolldown plugin timings 或 adapter-node d3 circular warnings。数据页优先复用 `DataToolbar` / `FilterPanel` / `DataTable`，再写页面特有业务单元格。
数据页状态优先复用 `src/lib/table-state.ts`：page size / offset、`sort_by` / `sort_dir`、column visibility、saved filters 都在该 helper 做规范化与 localStorage 持久化，页面只负责把状态映射到 API query。当前 `/admin/audit` 已验证服务端 pagination / sort，`/admin/users` 已验证 page size、列显隐与筛选持久化推广路径。

模板一致性审计：

```bash
node ../scripts/audit-page-templates.mjs
node ../scripts/audit-page-templates.mjs --json
```

审计覆盖所有 `src/routes/**/+page.svelte`，输出 header shell、toolbar / filter、table 模板、empty / loading / error 状态与缺口；`--fail-on-gaps` 可在后续迁移完成后接入 CI。

## 构建 warning 约束

- Markdown 渲染只注册少量 `highlight.js/lib/core` 语言，避免全量 `highlight.js` 打出超大 chunk。
- `build.rolldownOptions.checks.pluginTimings=false` 仅关闭 Rolldown 插件耗时提示，不关闭类型或 Svelte 诊断。
- `@xyflow/system` 显式列为 production dependency，使 adapter-node 在最终 server bundle 阶段外部化该包，避免把其 d3 依赖重新内联并打印已知 circular dependency warnings。

## 页面说明

| 路由 | 功能 |
|------|------|
| `/login` | 邮箱密码登录，成功后跳 `/orgs` |
| `/orgs` | 组织列表，支持切换激活 Org（X-Kooix-Org header）|
| `/orgs/[orgId]/projects` | 列出指定 Org 下的 Project，支持创建与 Org invite |
| `/orgs/[orgId]/projects/[projectId]` | Project 设置、Project invite、API Keys、模型别名 |
| `/invite/accept` | 公开邀请接受页：preview token、邮箱匹配、新用户设密码并加入 Org / Project |
| `/orgs/[orgId]/billing` | 月账单、quota alerts、CSV/JSON digest 导出与 invoice 状态机 |
| `/orgs/[orgId]/quotas` | Quota policy engine：org/project/api_key/user × model 策略、enforce/dry-run、explain 与 Redis/PG 对账 |
| `/channels` | Channel 列表与创建/编辑，plugin 渠道支持 Provider 插件预设与自定义 manifest |
| `/channels/[channelId]` | Channel 详情、key、健康状态、统计与调试信息 |
| `/admin/pricing` | Platform admin 定价规则管理，支持 global / channel-specific rules |
| `/admin/users` | Platform admin 用户生命周期管理：创建、停用/启用、重置密码、查看 / 撤销 refresh sessions |
| `/admin/sso` | Platform admin SSO Provider 管理：OIDC discovery、allowlist、auto-join role、redirect policy |
| `/admin/requests` / `/usage/requests` | 请求日志与使用明细过滤页 |

## API ID 约定

后端 response 已统一返回 typed ID（如 `org_...`、`proj_...`、`ch_...`、`usr_...`）。前端规则：

- 展示短 ID 用 `shortId()`。
- 构造 URL path 或 header 前，用 `rawId()` 转回裸 UUID；后端 `FlexUuid` 也能兼容 typed ID，但前端保持裸 UUID 可减少第三方链接歧义。
- 新增 API helper 时，不要手写 `split('_')`，统一从 `src/lib/id.ts` 引入工具。

## Provider 插件预设

Channel 表单在 `provider_type=plugin` 时提供预设下拉：

- 预设清单在 `src/lib/plugin-presets.ts`，当前 UI 覆盖 OpenAI-compatible、vLLM、LM Studio、Ollama OpenAI endpoint、LocalAI、Xinference、Anthropic Messages、Azure OpenAI、Gemini、DeepSeek、Mistral、Cohere、Ollama、Groq、Together、OpenRouter、Moonshot、智谱、通义千问、零一万物、Bedrock Converse；后端 manifest 也接受 `openai` alias。
- 选择预设会生成 `plugin.version = 1` manifest（含 capabilities / auth / preset），并展示 capability chips 与 Base URL 建议；旧 v0 `{ "plugin": { "preset": { "provider": "..." } } }` 仍由后端自动升级；自定义 manifest 仍可直接输入 JSON。
- Channel / Group 页面从 API 的 `capabilities` 字段展示能力；创建或编辑时会提示未声明的 image / audio / batch，路由层也会按 stream / tools / vision / JSON mode 跳过不满足能力的 channel。
- 测试在 `src/tests/plugin-presets.test.ts`，新增预设时同步补选项和测试。

## 技术选型

- SvelteKit 2.x（Svelte 5 runes 语法）
- TypeScript
- Tailwind CSS v4（`@tailwindcss/vite` 插件）
- 轻量 UI 组件（Button / Input / Card）手写，风格参考 shadcn
