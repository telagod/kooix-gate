# Web Components Index

> 所有页面级组件 / 复用组件的索引与约定。
>
> 设计语言基础：`web/src/lib/design/` 与 `web/src/app.css`（zinc-only + 语义色）
> 模板说明：`web/src/lib/design/README.md`

## 目录

```
web/src/lib/components/
├── brand/              # 品牌资产
├── channels/           # /channels 页面专用
├── flow/               # Playground / xyflow 节点
├── playground/         # Playground 容器
├── templates/          # 页面级模板（PageShell / DataTable / 等）
├── ui/                 # 基础 UI 原子（Button / Input / Card / 等）
├── InvitationPanel.svelte
├── Sidebar.svelte
├── Stat.svelte
└── Toast.svelte
```

## 1. `templates/` — 页面级模板

CLAUDE.md 强制：页面禁止复制基础按钮/输入框/卡片长 class。所有页面都走这些模板：

| 模板 | 用途 |
|------|------|
| `PageShell.svelte` | 标准页面容器（title / description / icon / actions / 内容区） |
| `AuthFrame.svelte` | 登录 / setup 页 |
| `SectionCard.svelte` | 卡片分组容器（带标题 / 描述 / 操作） |
| `StatePanel.svelte` | empty / loading / error 三态 |
| `ModalFrame.svelte` | 标准 modal（带遮罩 + ESC + 关闭按钮） |
| `DataToolbar.svelte` | 表格工具栏（搜索 + filter pills + actions） |
| `FilterPanel.svelte` | 高级筛选面板 |
| `DataTable.svelte` | 标准数据表（含分页 / 排序 / 列显隐 / 行选择） |

新页面起手：`PageShell` + `DataToolbar` + `DataTable` + `StatePanel` 三态。

## 2. `ui/` — 基础原子

| 组件 | 用途 |
|------|------|
| `Button.svelte` | 按钮（primary/secondary/ghost/destructive） |
| `Input.svelte` | 文本输入 |
| `Textarea.svelte` | 多行输入 |
| `Select.svelte` | 下拉选择 |
| `Field.svelte` | 表单字段（label + input + error） |
| `Card.svelte` | 卡片容器 |
| `Alert.svelte` | 提示条（info/warn/error/success） |
| `Badge.svelte` | 标签徽章 |
| `Skeleton.svelte` | 加载占位 |
| `DropdownMenu.svelte` | 下拉菜单 |
| `FilterPills.svelte` | 筛选条 |
| `MarkdownRenderer.svelte` | Markdown 渲染（lazy load `marked` + `highlight.js`） |
| `ModalityBadge.svelte` | 多模态能力徽章（chat/image/audio/embedding/vision） |
| `ProviderSelect.svelte` | Provider 选择（带 logo） |

## 3. `channels/` — 渠道管理专用

| 组件 | 用途 |
|------|------|
| `PluginAuthEditor.svelte` | Plugin manifest auth 编辑器（9 种 strategy） |

> ⚠ M1.4 计划：`channels/+page.svelte` (1949 行核弹) 拆为 `routes/channels/_components/{ChannelList,ChannelCreateDrawer,ManifestBuilder,SseReplayPreview,CapabilityChips}.svelte`。

## 4. `flow/` — Playground 节点

基于 `@xyflow/svelte`。每个节点是独立 Svelte 组件，input/output 通过 `Handle` 暴露。

| 节点 | Capability | 用途 |
|------|-----------|------|
| `BaseNode.svelte` | — | 节点基类（标题 / icon / status） |
| `TextInputNode.svelte` | — | 文本输入起点 |
| `ImageUploadNode.svelte` | — | 图片上传起点 |
| `AudioUploadNode.svelte` | — | 音频上传起点 |
| `LLMChatNode.svelte` | `chat` | LLM 对话节点 |
| `ImageGenNode.svelte` | `image` | 图像生成节点 |
| `STTNode.svelte` | `audio` | 语音转文本节点 |
| `TTSNode.svelte` | `audio` | 文本转语音节点 |
| `PreviewNode.svelte` | — | 结果预览终点 |

详见 `docs/playground.md`。

## 5. `playground/` — Playground 容器

| 组件 | 用途 |
|------|------|
| `FlowEditor.svelte` | 主编辑器容器（节点面板 / canvas / toolbar） |

入口 `web/src/routes/playground/+page.svelte` 用动态 import 懒加载 `FlowEditor`。

## 6. `brand/` — 品牌资产

| 组件 | 用途 |
|------|------|
| `KooixLogo.svelte` | Kooix 空衍 Logo（SVG inline） |

## 7. 顶层散件

| 组件 | 用途 |
|------|------|
| `Sidebar.svelte` | 主导航侧栏（带主题切换 / 用户菜单） |
| `Toast.svelte` | 全局 toast 通知（success/info/warn/error） |
| `Stat.svelte` | 单值统计卡（数值 + label + delta） |
| `InvitationPanel.svelte` | Org/Project 邀请面板（管理 + 接受） |

## 约定

- **Icon**：lucide-svelte（通用） / `web/static/providers/{slug}.svg`（Provider logo）
- **配色**：zinc-only + 语义色（green/amber/red），不用 blue/purple/indigo 装饰色
- **样式**：Tailwind v4 + `cn()` (clsx + tailwind-merge from `$lib/design`)
- **组件类型**：`Svelte 5 runes`（`$state` / `$derived` / `$props` / `$bindable`）
- **门禁**：`npm run check` 必须 0 errors / 0 warnings
- **bundle**：单页 ≤ 500 行；超过的拆 `_components/`
