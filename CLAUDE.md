# Kooix Gate — Project Rules

## Rust 后端

- 编译期 SQL 校验：sqlx + `cargo sqlx prepare`，不用 ORM
- 强类型 ID：`OrgId` / `ProjectId` 等编译期防串台
- API response 返带前缀 typed ID（如 `org_019e2c1ba7d17162842207e4b24f5f98`）
- URL 路径参数用 `FlexUuid`，同时接受 typed ID 和裸 UUID
- 新增 migration 后跨 crate 测试要先 `cargo clean -p gate-storage`
- testcontainers 默认 `postgres:17-alpine`，可用 `KOOIX_TEST_PG_TAG` 覆盖

## 前端设计规范（web/）

### 技术栈

- SvelteKit 2.x + Svelte 5 runes（`$state`, `$derived`, `$props`, `$bindable`）
- Tailwind CSS v4（`@tailwindcss/vite`）
- TypeScript strict

### Icon 规范

**禁止 emoji 作为 UI icon。**

- **通用 icon**：统一使用 `lucide-svelte`
  ```svelte
  import { Bot, Globe, Zap } from 'lucide-svelte';
  <Bot size={16} />
  ```
- **Provider logo**：使用 `web/static/providers/{slug}.svg` 真实品牌 logo
  ```svelte
  <img src="/providers/openai.svg" alt="OpenAI" class="w-4 h-4 dark:invert" />
  ```
  - 来源：simple-icons（devDep）提取 + 手工补充缺失品牌
  - dark mode 用 CSS `dark:invert` 而非 `currentColor`（`<img>` 无法继承 CSS 变量）
  - SVG 格式：`viewBox="0 0 24 24"`，黑色填充，无 `fill` 属性或 `fill="#000"`
  - 新增 provider 时：先查 `node_modules/simple-icons/icons/`，没有则手工创建 monogram SVG

- 标准尺寸：inline `size={16}` / `w-4 h-4`，card/grid `size={20}` / `w-5 h-5`，hero `size={24}` / `w-6 h-6`
- lucide icon 颜色跟随 text color class，不单独设色

### 调色板

- 单色系：zinc only（详见 `web/src/app.css` Design System Tokens）
- 语义色仅用于健康指示（green）、警告（amber）、错误/破坏性操作（red）
- 不使用 blue / purple / indigo 等装饰色

### 文字对比度

| 场景 | Light | Dark | 对比度 |
|------|-------|------|--------|
| 主文字 | zinc-900 | zinc-100 | 15-17:1 |
| 次要文字 | zinc-600 | zinc-300 | 7-10:1 |
| 弱文字/标签 | zinc-500 | zinc-400 | 4.6-5.9:1 |
| 占位/禁用 | zinc-500 | zinc-400 | 同上 |
| 装饰性 icon | zinc-400 | zinc-500 | 非文字，3:1 OK |

### 组件约定

- 手写轻量组件（Button / Input / Card / Skeleton / ProviderSelect），风格参考 shadcn
- 组件放 `web/src/lib/components/ui/`，页面级组件直接放路由
- 使用 `clsx` 做条件 class 拼接
- 字体：Inter（sans）+ JetBrains Mono（mono），在 `app.css @theme` 配置

### 国际化

- UI 文案中文为主，术语保留英文（如 Provider、Channel、Healthy）
- API 字段名全英文
