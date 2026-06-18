<script lang="ts">
	import { onMount } from 'svelte';
	import {
		forceCooldownChannel,
		getHealthDashboard,
		unbanChannel,
		type ChannelHealthResponse,
		type DashboardSummary,
		type HealthState
	} from '$lib/api.js';
	import { Badge, Button, Card } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { dataTemplate } from '$lib/design';
	import {
		Activity,
		AlertTriangle,
		HeartPulse,
		RefreshCw,
		ShieldAlert,
		ShieldCheck,
		Snowflake,
		TriangleAlert
	} from 'lucide-svelte';

	let summary = $state<DashboardSummary | null>(null);
	let loading = $state(true);
	let error = $state('');
	let actionMessage = $state('');

	async function refresh() {
		loading = true;
		error = '';
		try {
			summary = await getHealthDashboard();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	onMount(refresh);

	const stateMeta: Record<
		HealthState,
		{ label: string; color: string; icon: typeof HeartPulse }
	> = {
		healthy: { label: '健康', color: 'text-green-600 dark:text-green-400', icon: ShieldCheck },
		degraded: {
			label: '降级',
			color: 'text-amber-600 dark:text-amber-400',
			icon: AlertTriangle
		},
		cooldown: { label: '冷却', color: 'text-blue-600 dark:text-blue-400', icon: Snowflake },
		recovering: {
			label: '恢复中',
			color: 'text-violet-600 dark:text-violet-400',
			icon: Activity
		},
		banned: { label: '封禁', color: 'text-red-600 dark:text-red-400', icon: ShieldAlert }
	};

	async function handleUnban(channelId: string) {
		actionMessage = '';
		try {
			await unbanChannel(channelId);
			actionMessage = `已解封 ${channelId.slice(0, 8)}…，状态置为 Cooldown 进入探针窗口`;
			await refresh();
		} catch (e) {
			actionMessage = `解封失败：${e instanceof Error ? e.message : String(e)}`;
		}
	}

	async function handleForceCooldown(channelId: string) {
		actionMessage = '';
		try {
			await forceCooldownChannel(channelId, { cooldown_secs: 300 });
			actionMessage = `已强制冷却 ${channelId.slice(0, 8)}… 5 分钟`;
			await refresh();
		} catch (e) {
			actionMessage = `强制冷却失败：${e instanceof Error ? e.message : String(e)}`;
		}
	}

	function fmtScore(s: number): string {
		return s.toFixed(2);
	}
	function fmtPct(s: number): string {
		return `${(s * 100).toFixed(1)}%`;
	}
	function isAlertChannel(c: ChannelHealthResponse): boolean {
		return c.state === 'banned' || c.state === 'cooldown' || c.state === 'recovering';
	}
</script>

<PageShell
	title="号池健康仪表盘"
	description="ADR-0007 ChannelHealthScore：5 状态分布、渠道健康度排名、告警列表与运维快速操作。"
	icon={HeartPulse}
>
	<div class="flex justify-end mb-4 gap-2">
		<Button onclick={refresh} disabled={loading}>
			<RefreshCw size={14} />
			刷新
		</Button>
	</div>

	{#if actionMessage}
		<div
			class="mb-4 px-3 py-2 rounded border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-950 text-sm"
		>
			{actionMessage}
		</div>
	{/if}

	{#if error}
		<StatePanel variant="danger" title="加载失败" description={error} />
	{:else if loading && !summary}
		<StatePanel title="正在加载仪表盘..." />
	{:else if summary}
		<!-- 5 状态分布卡片 -->
		<div class="grid grid-cols-2 md:grid-cols-5 gap-3 mb-6">
			{#each Object.entries(stateMeta) as [stateKey, meta] (stateKey)}
				{@const count = summary.by_state[stateKey] ?? 0}
				<Card class="p-4">
					<div class="flex items-center gap-2 mb-2">
						<meta.icon size={14} class={meta.color} />
						<span class="text-xs text-zinc-500 dark:text-zinc-400">{meta.label}</span>
					</div>
					<div class="text-2xl font-semibold {meta.color}">{count}</div>
				</Card>
			{/each}
		</div>

		<!-- 告警列表 -->
		{#if summary.alerts.length > 0}
			<Card class="p-4 mb-6">
				<h2 class="text-base font-medium mb-3 flex items-center gap-2">
					<TriangleAlert size={16} class="text-amber-500" />
					告警 ({summary.alerts.length})
				</h2>
				<ul class="space-y-2">
					{#each summary.alerts as alert, i (alert.channel_id + i)}
						<li
							class="flex items-start gap-3 text-sm border-l-2 pl-3 {alert.severity ===
							'critical'
								? 'border-red-500'
								: 'border-amber-500'}"
						>
							<Badge variant={alert.severity === 'critical' ? 'danger' : 'warning'}>
								{alert.severity === 'critical' ? '严重' : '警告'}
							</Badge>
							<div class="flex-1">
								<div class="font-medium">{alert.channel_code}</div>
								<div class="text-zinc-500 dark:text-zinc-400">{alert.message}</div>
							</div>
						</li>
					{/each}
				</ul>
			</Card>
		{/if}

		<!-- 渠道健康度表 -->
		<Card class="p-4">
			<h2 class="text-base font-medium mb-3 flex items-center gap-2">
				<Activity size={16} />
				渠道健康度 ({summary.channels.length})
			</h2>
			<div class={dataTemplate.tableWrap}>
				<table class={dataTemplate.table}>
					<thead class={dataTemplate.head}>
						<tr>
							<th class={dataTemplate.th}>渠道</th>
							<th class={dataTemplate.th}>状态</th>
							<th class={dataTemplate.th}>分数</th>
							<th class={dataTemplate.th}>成功率</th>
							<th class={dataTemplate.th}>P99 延迟 ms</th>
							<th class={dataTemplate.th}>窗口</th>
							<th class={dataTemplate.th}>连续失败</th>
							<th class={dataTemplate.th}>操作</th>
						</tr>
					</thead>
					<tbody class={dataTemplate.body}>
						{#each summary.channels as c (c.channel_id)}
							{@const meta = stateMeta[c.state]}
							<tr class={dataTemplate.row}>
								<td class={dataTemplate.td}>
									<div class="font-medium">{c.channel_code}</div>
									<div class="text-xs text-zinc-500">{c.channel_id.slice(0, 16)}…</div>
								</td>
								<td class={dataTemplate.td}>
									<span class="inline-flex items-center gap-1 {meta.color}">
										<meta.icon size={12} />
										{meta.label}
									</span>
								</td>
								<td class={dataTemplate.td}>{fmtScore(c.score)}</td>
								<td class={dataTemplate.td}>{fmtPct(c.success_rate)}</td>
								<td class={dataTemplate.td}>{c.latency_p99_ms}</td>
								<td class={dataTemplate.td}>
									{c.window_success}/{c.window_total}
								</td>
								<td class={dataTemplate.td}>
									<span
										class={c.consecutive_failures > 0 ? 'text-red-600 dark:text-red-400' : ''}
									>
										{c.consecutive_failures}
									</span>
								</td>
								<td class={dataTemplate.td}>
									<div class="flex gap-1">
										{#if c.state === 'banned'}
											<Button onclick={() => handleUnban(c.channel_id)}>解封</Button>
										{/if}
										{#if isAlertChannel(c) === false}
											<Button onclick={() => handleForceCooldown(c.channel_id)}>
												强制冷却
											</Button>
										{/if}
									</div>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</Card>
	{/if}
</PageShell>
