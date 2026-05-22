<script lang="ts">
	// 0.4.15：从 admin/users/+page.svelte 抽出的 session 列表 modal。
	import { Alert, Badge, Button, Card, Skeleton } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { LogOut, MonitorSmartphone, RefreshCcw } from 'lucide-svelte';
	import type { UserDetail, UserSession } from '$lib/api.js';

	interface Props {
		sessionTarget: UserDetail | null;
		sessions: UserSession[];
		sessionsLoading: boolean;
		sessionError: string;
		revokeAllBusy: boolean;
		sessionBusy: Record<string, boolean>;
		text: { primary: string; secondary: string; muted: string };
		onClose: () => void;
		onRefresh: () => void | Promise<void>;
		onRevokeAll: () => void | Promise<void>;
		onRevokeSession: (session: UserSession) => void | Promise<void>;
	}

	let {
		sessionTarget,
		sessions,
		sessionsLoading,
		sessionError,
		revokeAllBusy,
		sessionBusy,
		text,
		onClose,
		onRefresh,
		onRevokeAll,
		onRevokeSession,
	}: Props = $props();

	function formatTimestamp(s: string | null | undefined): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleString('zh-CN', {
				month: '2-digit',
				day: '2-digit',
				hour: '2-digit',
				minute: '2-digit',
			});
		} catch {
			return s;
		}
	}
</script>

{#if sessionTarget}
	<ModalFrame close={onClose} class="bg-zinc-950/40" panelClass="w-full max-w-4xl">
		<Card padding="lg" class="max-h-[85vh] w-full max-w-4xl overflow-y-auto">
			<div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
				<div class="flex items-center gap-2">
					<MonitorSmartphone size={18} class={text.secondary} />
					<div>
						<p class="font-semibold {text.primary}">活跃 refresh sessions 会话</p>
						<p class="text-xs {text.muted}">{sessionTarget.email} · 撤销后仅阻断后续 refresh，已签发 access token 会自然过期。</p>
					</div>
				</div>
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={onRefresh} disabled={sessionsLoading}>
						<RefreshCcw size={12} class={sessionsLoading ? 'animate-spin' : ''} />刷新
					</Button>
					<Button variant="destructive" size="sm" onclick={onRevokeAll} disabled={revokeAllBusy || sessions.length === 0}>
						<LogOut size={12} />全部踢下线
					</Button>
				</div>
			</div>

			{#if sessionError}
				<Alert variant="danger" class="mb-3">{sessionError}</Alert>
			{/if}

			{#if sessionsLoading}
				<div class="space-y-2">
					{#each Array(3) as _}
						<Skeleton class="h-14" />
					{/each}
				</div>
			{:else if sessions.length === 0}
				<StatePanel variant="default" title="无活跃 session" description="该用户当前没有有效的 refresh token。" />
			{:else}
				<div class="space-y-2">
					{#each sessions as session}
						<div class="flex flex-col gap-2 rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-700 dark:bg-zinc-900 sm:flex-row sm:items-center sm:justify-between">
							<div class="space-y-1">
								<div class="flex items-center gap-2">
									<Badge variant={session.current ? 'default' : 'success'}>
										{#if session.current}当前会话{:else}其它设备{/if}
									</Badge>
									<span class="text-xs font-mono {text.muted}">id={session.id.slice(0, 8)}</span>
								</div>
								<div class="grid grid-cols-1 gap-1 text-xs sm:grid-cols-2 {text.secondary}">
									<span>IP：<span class="font-mono">{session.ip ?? '—'}</span></span>
									<span>UA：<span class="font-mono truncate">{session.user_agent ?? '—'}</span></span>
									<span>创建：{formatTimestamp(session.created_at)}</span>
									<span>过期：{formatTimestamp(session.expires_at)}</span>
									<span>最近活跃：{formatTimestamp(session.last_used_at)}</span>
								</div>
							</div>
							<Button
								variant="outline"
								size="sm"
								class="self-end"
								disabled={sessionBusy[session.id]}
								onclick={() => onRevokeSession(session)}
							>
								<LogOut size={12} />{sessionBusy[session.id] ? '撤销中...' : '撤销'}
							</Button>
						</div>
					{/each}
				</div>
			{/if}

			<div class="mt-5 flex justify-end">
				<Button variant="ghost" onclick={onClose}>关闭</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
