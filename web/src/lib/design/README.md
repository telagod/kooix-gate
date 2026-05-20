# Kooix Gate 前端设计模板

目标：把控制台的视觉语言从页面散落的 Tailwind class 抽到 `$lib/design` 与 `$lib/components/templates`，页面只表达业务结构。

## 分层

- `web/src/lib/design/classes.ts`：设计 token 的 TypeScript 门面，集中维护 `text`、`surface`、`buttonClass`、`cardClass`、`controlClass`、`badgeClass`、`alertClass`、`pageTemplate`、`authTemplate`。
- `web/src/lib/components/ui/`：原子 UI，消费 `$lib/design`，不在页面重复基础按钮、输入框、卡片、状态徽标样式。
- `web/src/lib/components/templates/`：页面模板，承载通用布局与美学节奏。
- `web/src/routes/**`：业务页面，可保留数据表、图表、特殊流程的局部 class，但新页面默认先用模板。

## 模板

| 模板 | 用途 | 默认美学 |
| --- | --- | --- |
| `PageShell` | 控制台内容页：标题、描述、右侧 actions、最大宽度 | `p-6`、zinc-only、统一 H1 |
| `AuthFrame` | 登录/初始化等无 sidebar 页面 | 居中卡片、主题按钮 slot |
| `SectionCard` | 表单区块、设置区块、信息组 | Card + 标题行 + icon + actions |
| `StatePanel` | 空态、错误态、无权限态 | 居中 Card，按 danger/warning/success 控制语义色 |
| `ModalFrame` | Modal / drawer 遮罩与 outside click | 可访问的全屏遮罩 button + 独立 panel slot |
| `DataToolbar` | 数据页搜索、quick filters、actions、active badges | `query` / `controls` / `actions` / `badges` snippets，统一 flex wrap 节奏 |
| `FilterPanel` | 高级筛选面板 | zinc-only 边框面板，`open` 控制显隐 |
| `DataTable` | 数据表容器、表头、空态、footer | 统一 table wrapper / thead / tbody / empty cell；配合 `dataTemplate` class token |

## 新页面基线

```svelte
<script lang="ts">
	import { Button, Field, Input } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import SectionCard from '$lib/components/templates/SectionCard.svelte';
	import { Settings } from 'lucide-svelte';
</script>

<PageShell title="页面标题" description="一句话说明当前任务。" icon={Settings}>
	{#snippet actions()}
		<Button size="sm">新建</Button>
	{/snippet}

	<SectionCard title="配置" description="表单说明。">
		<Field label="名称" for="demo-name">
			<Input id="demo-name" placeholder="value" />
		</Field>
	</SectionCard>
</PageShell>
```

## 收口状态

- 当前设计迁移已清掉旧页面的 Svelte a11y / deprecation warning；`npm run check` 应保持 `0 errors and 0 warnings`。
- `ModalFrame` 已用于旧 modal/drawer 的遮罩收敛，后续新增弹层不要再手写 `div onclick` 遮罩。
- `PageShell` / `AuthFrame` / `StatePanel` / `DataToolbar` / `FilterPanel` / `DataTable` 已作为页面与数据页模板落地，并迁移 `setup`、`login`、`invite/accept`、`admin/audit`、`admin/users`、`admin/groups`、`admin/incidents`、`admin/pricing`、`admin/requests`、`admin/sso`、`orgs/[orgId]/quotas`、`orgs/[orgId]/billing`、`orgs/[orgId]/projects`、`orgs/[orgId]/projects/[projectId]`、`orgs/[orgId]/projects/[projectId]/keys`、`usage/+page`、`usage/requests`、`channels/+page`、`channels/[channelId]` 代表页；后续新增数据页继续优先复用模板。
- 表格状态基座在 `src/lib/table-state.ts`；server-side pagination / sort / column visibility / saved filters 不要在 route 内重复造 localStorage key 与 normalize 逻辑。

## 美学约束

- 装饰色禁用 blue / purple / indigo；品牌 logo 用 `web/static/providers/*.svg`。
- 语义色只用于 success/warning/danger/health，不用于装饰。
- 文字对比沿用 `text.primary` / `text.secondary` / `text.muted`。
- 交互 class 优先用 `buttonClass`、`controlClass`、`cardClass`、`badgeClass`，避免手写重复长串。
- 新增模板前先确认它表达的是布局节奏，不是单个页面的业务细节。
