<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { acceptInvitation, previewInvitation } from '$lib/api.js';
	import type { InvitationPreview } from '$lib/api.js';
	import AuthFrame from '$lib/components/templates/AuthFrame.svelte';
	import { Alert, Badge, Button, Card, Field, Input } from '$lib/components/ui';
	import { Check, Mail, UserPlus } from 'lucide-svelte';

	let token = $state('');
	let preview = $state<InvitationPreview | null>(null);
	let loading = $state(true);
	let accepting = $state(false);
	let error = $state('');
	let success = $state('');
	let form = $state({ email: '', display_name: '', password: '' });

	onMount(async () => {
		token = $page.url.searchParams.get('token') ?? '';
		if (!token) {
			error = '缺少 invitation token';
			loading = false;
			return;
		}
		try {
			preview = await previewInvitation(token);
			form.email = preview.email;
		} catch (err: any) {
			error = err?.message ?? '邀请不可用';
		} finally {
			loading = false;
		}
	});

	async function submitAccept() {
		if (!preview) return;
		accepting = true;
		error = '';
		success = '';
		try {
			await acceptInvitation({
				token,
				email: form.email.trim(),
				display_name: form.display_name.trim() || undefined,
				password: form.password || undefined
			});
			success = '邀请已接受，请登录控制台';
			setTimeout(() => goto('/login'), 1200);
		} catch (err: any) {
			error = err?.message ?? '接受邀请失败';
		} finally {
			accepting = false;
		}
	}

	function fmtDate(value: string): string {
		return new Date(value).toLocaleString('zh-CN');
	}
</script>

<AuthFrame title="接受邀请" description="加入 Kooix Gate 组织或项目" max="md">
	{#if loading}
		<Card padding="md" class="text-sm text-zinc-600 dark:text-zinc-300">加载邀请...</Card>
	{:else if error && !preview}
		<Alert variant="danger">{error}</Alert>
	{:else if preview}
		<Card padding="md" class="space-y-5">
			<div class="flex items-start gap-3">
				<div class="flex h-10 w-10 items-center justify-center rounded-lg bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900">
					<UserPlus size={18} />
				</div>
				<div>
					<div class="flex flex-wrap items-center gap-2">
						<h1 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">{preview.email}</h1>
						<Badge variant={preview.status === 'pending' ? 'success' : 'warning'}>{preview.status}</Badge>
					</div>
					<p class="mt-1 text-sm text-zinc-600 dark:text-zinc-300">
						{preview.scope_kind} · {preview.role} · 过期 {fmtDate(preview.expires_at)}
					</p>
				</div>
			</div>

			{#if preview.status !== 'pending'}
				<Alert variant="warning">该邀请不是 pending 状态，无法继续接受。</Alert>
			{:else}
				<div class="space-y-3">
					<Field label="邮箱" for="invite-email">
						<Input id="invite-email" bind:value={form.email} disabled />
					</Field>
					<Field label="显示名" for="invite-name">
						<Input id="invite-name" bind:value={form.display_name} placeholder="可选" disabled={accepting} />
					</Field>
					<Field label="密码" for="invite-password" hint="如果邮箱已存在，可留空；新用户必须设置。">
						<Input id="invite-password" type="password" bind:value={form.password} placeholder="至少 8 个字符" disabled={accepting} />
					</Field>
				</div>

				{#if error}
					<Alert variant="danger">{error}</Alert>
				{/if}
				{#if success}
					<Alert variant="success">
						<div class="flex items-center gap-2"><Check size={14} /> {success}</div>
					</Alert>
				{/if}

				<Button onclick={submitAccept} disabled={accepting} class="w-full">
					<Mail size={14} />
					{accepting ? '接受中...' : '接受邀请'}
				</Button>
			{/if}
		</Card>
	{/if}
</AuthFrame>
