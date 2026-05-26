# DataTable Virtualization 设计稿

> Status: **设计稿（0.4.115）→ 0.5.x 实装**
> 关联：[product-review-followup-2026-05-26 §1](./product-review-followup-2026-05-26.md) | [product-gaps.md G-106](./product-gaps.md#g-106-web-bundle-220--180-kb)

## 当前状态

`web/src/lib/components/templates/DataTable.svelte` 当前是 **snippet-passthrough 模板**：

```svelte
<DataTable head={head_snippet} children={rows_snippet} />
```

Caller 自己渲染所有 row。0.4.85 加了 `maxHeight` + `stickyHead` 两个 prop，**但 DOM 仍然渲染所有行**——admin/requests 万行 request log 时浏览器会卡死（每行至少 5 个 cell + 6-8 个 inline expression）。

## 真正的虚拟化（windowing）需要什么

DataTable 接管 row 渲染，从 caller 拿：

1. **rows: T[]** — 全数据
2. **rowSnippet: Snippet<[row: T, index: number]>** — 渲染单行（caller 提供 cell 模板）
3. **rowHeight: number** — 每行高度（assume 等高，简化 layout 计算）
4. **overscan: number = 5** — 视口外预渲染数量

DataTable 内部用 IntersectionObserver 或 scroll listener 计算可见 row 索引，只 mount [start, end] 范围的 row。

## 接口契约（完整）

```svelte
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		// 现有 props（兼容）
		class: className = '',
		tableClass = '',
		headClass = '',
		bodyClass = '',
		footerClass = '',
		isEmpty = false,
		emptyColspan = 1,
		maxHeight = '',
		stickyHead = false,
		head,
		empty,
		footer,
		// 新 props（虚拟化模式）
		rows = undefined,           // 启用虚拟化时传
		rowSnippet = undefined,
		rowHeight = 48,             // 默认 48px (符合 zinc design)
		overscan = 5,
		children                    // legacy 模式继续支持
	}: {
		// ... 现有
		rows?: any[];
		rowSnippet?: Snippet<[row: any, index: number]>;
		rowHeight?: number;
		overscan?: number;
		children?: Snippet;
	} = $props();

	// 模式选择：传 rows + rowSnippet → 虚拟化；传 children → legacy
	const isVirtual = $derived(!!rows && !!rowSnippet);
</script>
```

## Layout 算法

```svelte
{#if isVirtual}
	<div class="overflow-y-auto" style="max-height: {maxHeight || '100%'}" bind:this={scrollContainer}>
		<table>
			<!-- thead 同 legacy -->
			<tbody>
				<!-- 顶部 spacer：撑出已滚过的行的占位高度 -->
				<tr style="height: {visibleStart * rowHeight}px"><td colspan="∞"></td></tr>

				<!-- 真正渲染的 row（只有 [start, end]） -->
				{#each rows.slice(visibleStart, visibleEnd) as row, idx}
					<tr style="height: {rowHeight}px">
						{@render rowSnippet(row, visibleStart + idx)}
					</tr>
				{/each}

				<!-- 底部 spacer：撑出未滚到的行的占位高度 -->
				<tr style="height: {(rows.length - visibleEnd) * rowHeight}px"><td colspan="∞"></td></tr>
			</tbody>
		</table>
	</div>
{:else}
	<!-- legacy passthrough 模式 -->
{/if}
```

`visibleStart` / `visibleEnd` 用 `$derived` 从 scrollTop + viewportHeight + rowHeight 算：

```svelte
let scrollTop = $state(0);
const viewportHeight = $derived(scrollContainer?.clientHeight ?? 600);
const visibleStart = $derived(
	Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
);
const visibleEnd = $derived(
	Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan)
);
```

## 已知限制

1. **必须等高**：rowHeight 是固定值。变高 row（如 expanded request 详情）需要 mode `rowHeight = 'auto' | number`，但 'auto' 模式需要 ResizeObserver 测每行实际高度，复杂度上升 3 倍。**v1 不做**。
2. **不能用于 `<table>` flexbox**：spacer tr + rowspan="∞" 在 table layout 中是 hack，部分浏览器（Safari < 16）可能 layout 错乱。备选：用 `display: block` + `<div role="row">` 替代 `<tr>`，但要重写所有 caller 的 cell 模板。**v1 仍用 tr**。
3. **search/filter 不在范围内**：DataTable 拿到的 rows 已经是 filter 后的结果，virtualize 不感知。filter 在 caller。
4. **column resize**：v1 不做。

## Caller 改造（admin/requests 案例）

```svelte
<!-- 当前 -->
<DataTable>
	{#snippet head()}<tr>...</tr>{/snippet}
	{#each filteredRows as row}
		<tr>...</tr>
	{/each}
</DataTable>

<!-- 新 -->
<DataTable
	rows={filteredRows}
	rowHeight={56}
	maxHeight="600px"
	stickyHead
>
	{#snippet head()}<tr>...</tr>{/snippet}
	{#snippet rowSnippet(row, idx)}
		<td>{row.timestamp}</td>
		<td>{row.model}</td>
		<!-- ... -->
	{/snippet}
</DataTable>
```

**Migration cost**：每个 caller 改 1 处 `{#each}` → `{#snippet rowSnippet}`。当前 caller 是 admin/requests / admin/audit / admin/incidents / admin/groups（万行 + 千行 + 百行 + 数十行）。前两个必须迁；后两个可继续 legacy。

## 性能预算

| 数据规模 | legacy（每 row 真渲染） | virtualize (rowHeight=56, viewport=600) |
|---------|------------------------|----------------------------------------|
| 100 rows | <50ms 首屏，无卡顿 | <50ms 首屏，无 win |
| 1000 rows | ~300ms 首屏，滚动 30fps | <50ms 首屏，滚动 60fps |
| 10000 rows | ~3s 首屏（用户感知卡死），滚动 5-10fps | <50ms 首屏，滚动 60fps |
| 100000 rows | 浏览器 OOM | <50ms 首屏，滚动 60fps，内存 <50MB |

## 验收门禁（v0.5.x 实装时）

- [ ] DataTable 兼容 legacy mode（无 rows prop 时退到 children passthrough，现有 caller 0 改动）
- [ ] virtualize mode：admin/requests 10k 假数据滚动 60fps（用 Performance API timing）
- [ ] sticky head 在虚拟化模式仍生效
- [ ] focus / scrollIntoView / Ctrl+F 浏览器原生搜索仍可用（虚拟化破坏的话需补 fallback）
- [ ] e2e test：admin/requests 模拟 1000 rows，滚到末尾后再滚回首部，row 内容一致

## 不做什么（v1 范围外）

- 变高 row（rowHeight='auto'）
- column resize / reorder
- 多选 selection 跨虚拟化边界（要追加 mounted-row → checked map sync）
- 横向虚拟化（列虚拟化，仅当 column 数 > 50 时考虑）

## 决策原因

第一刀 0.4.85 加 sticky head 解决"看不见列名"，但 admin/requests 万行真实数据下浏览器仍会卡死。本设计：

1. 锁住 DataTable v1 接口（rows + rowSnippet + rowHeight + overscan），让现有 caller migration cost = 1 处 each → snippet
2. 等高假设让 layout 算法极简（spacer tr + slice），避免 ResizeObserver 复杂度
3. legacy mode 保留，避免破坏百行 / 数十行 caller

实装在 v0.5.x（涉及 row index 计算 + scroll throttle + spacer DOM + 测试 fixture，超出 patch 范围）。

---

*Designer: 邪修红尘仙 / Date: 2026-05-26 / 关联 commit: 0.4.115*
