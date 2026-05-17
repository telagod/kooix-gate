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

## 构建 warning 约束

- Markdown 渲染只注册少量 `highlight.js/lib/core` 语言，避免全量 `highlight.js` 打出超大 chunk。
- `build.rolldownOptions.checks.pluginTimings=false` 仅关闭 Rolldown 插件耗时提示，不关闭类型或 Svelte 诊断。
- `@xyflow/system` 显式列为 production dependency，使 adapter-node 在最终 server bundle 阶段外部化该包，避免把其 d3 依赖重新内联并打印已知 circular dependency warnings。

## 页面说明

| 路由 | 功能 |
|------|------|
| `/login` | 邮箱密码登录，成功后跳 `/orgs` |
| `/orgs` | 组织列表，支持切换激活 Org（X-Kooix-Org header）|
| `/orgs/[orgId]/projects` | 列出指定 Org 下的 Project，支持创建 |

## 技术选型

- SvelteKit 2.x（Svelte 5 runes 语法）
- TypeScript
- Tailwind CSS v4（`@tailwindcss/vite` 插件）
- 轻量 UI 组件（Button / Input / Card）手写，风格参考 shadcn
