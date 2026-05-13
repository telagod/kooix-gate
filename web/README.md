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
