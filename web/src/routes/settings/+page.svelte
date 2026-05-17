<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, changePassword } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import { Button, Field, Input, Skeleton } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import SectionCard from '$lib/components/templates/SectionCard.svelte';
	import { User, Lock, Shield, Settings } from 'lucide-svelte';

	let me = $state<MeResult | null>(null);
	let loading = $state(true);

	let currentPw = $state('');
	let newPw = $state('');
	let confirmPw = $state('');
	let pwSaving = $state(false);
	let pwMsg = $state('');
	let pwError = $state('');

	onMount(async () => {
		try { me = await getMe(); } catch {}
		loading = false;
	});

	async function handleChangePw() {
		pwError = '';
		pwMsg = '';
		if (newPw.length < 8) { pwError = '密码至少 8 位'; return; }
		if (newPw !== confirmPw) { pwError = '两次输入不一致'; return; }
		pwSaving = true;
		try {
			await changePassword(currentPw, newPw);
			pwMsg = '密码已修改';
			currentPw = '';
			newPw = '';
			confirmPw = '';
		} catch (err: any) {
			pwError = err?.message ?? '修改失败';
		} finally {
			pwSaving = false;
		}
	}
</script>

<PageShell title="个人设置" description="账号信息、密码和当前组织上下文。" max="narrow" icon={Settings}>
	{#if loading}
		<div class="space-y-4">
			{#each Array(2) as _}
				<Skeleton class="h-32" />
			{/each}
		</div>
	{:else}
		<SectionCard title="账号信息" icon={User} class="mb-6">
			{#if me}
				<div class="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
					<div>
						<p class="text-xs font-medium text-zinc-600 dark:text-zinc-300 mb-1">用户 ID</p>
						<p class="font-mono text-zinc-900 dark:text-zinc-100">{me.subject?.user_id ?? '—'}</p>
					</div>
					<div>
						<p class="text-xs font-medium text-zinc-600 dark:text-zinc-300 mb-1">角色</p>
						<p class="text-zinc-900 dark:text-zinc-100">
							{#if me.is_platform_admin}
								<span class="inline-flex items-center gap-1">
									<Shield size={12} class="text-amber-500" /> Platform Admin
								</span>
							{:else}
								普通用户
							{/if}
						</p>
					</div>
					<div>
						<p class="text-xs font-medium text-zinc-600 dark:text-zinc-300 mb-1">所属组织</p>
						<p class="text-zinc-900 dark:text-zinc-100">{me.orgs?.length ?? 0} 个</p>
					</div>
					<div>
						<p class="text-xs font-medium text-zinc-600 dark:text-zinc-300 mb-1">当前组织</p>
						<p class="font-mono text-zinc-900 dark:text-zinc-100">{me.current_org?.slice(0, 12) ?? '—'}...</p>
					</div>
				</div>
			{/if}
		</SectionCard>

		<SectionCard title="修改密码" icon={Lock}>
			<div class="space-y-3 max-w-sm">
				<Field label="当前密码" for="current-password">
					<Input id="current-password" type="password" autocomplete="current-password" bind:value={currentPw} />
				</Field>
				<Field label="新密码" for="new-password">
					<Input id="new-password" type="password" autocomplete="new-password" bind:value={newPw} />
				</Field>
				<Field label="确认新密码" for="confirm-password">
					<Input id="confirm-password" type="password" autocomplete="new-password" bind:value={confirmPw} />
				</Field>
				{#if pwError}
					<p class="text-xs text-red-600 dark:text-red-400">{pwError}</p>
				{/if}
				{#if pwMsg}
					<p class="text-xs text-green-600 dark:text-green-400">{pwMsg}</p>
				{/if}
				<Button size="sm" onclick={handleChangePw} disabled={pwSaving}>
					{pwSaving ? '保存中...' : '修改密码'}
				</Button>
			</div>
		</SectionCard>
	{/if}
</PageShell>
