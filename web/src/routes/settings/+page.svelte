<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, changePassword } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import { User, Lock, Shield } from 'lucide-svelte';

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

<div class="max-w-3xl mx-auto p-6">
	<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 mb-6">个人设置</h1>

	{#if loading}
		<div class="space-y-4">
			{#each Array(2) as _}
				<div class="h-32 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
			{/each}
		</div>
	{:else}
		<!-- Profile info -->
		<Card class="p-5 mb-6">
			<div class="flex items-center gap-2 mb-4">
				<User size={16} class="text-zinc-400" />
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">账号信息</h2>
			</div>
			{#if me}
				<div class="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
					<div>
						<p class="text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">用户 ID</p>
						<p class="font-mono text-zinc-900 dark:text-zinc-100">{me.subject?.user_id ?? '—'}</p>
					</div>
					<div>
						<p class="text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">角色</p>
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
						<p class="text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">所属组织</p>
						<p class="text-zinc-900 dark:text-zinc-100">{me.orgs?.length ?? 0} 个</p>
					</div>
					<div>
						<p class="text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">当前组织</p>
						<p class="font-mono text-zinc-900 dark:text-zinc-100">{me.current_org?.slice(0, 12) ?? '—'}...</p>
					</div>
				</div>
			{/if}
		</Card>

		<!-- Change password -->
		<Card class="p-5">
			<div class="flex items-center gap-2 mb-4">
				<Lock size={16} class="text-zinc-400" />
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">修改密码</h2>
			</div>
			<div class="space-y-3 max-w-sm">
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">当前密码</label>
					<input type="password" bind:value={currentPw} class="w-full h-10 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100" />
				</div>
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">新密码</label>
					<input type="password" bind:value={newPw} class="w-full h-10 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100" />
				</div>
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">确认新密码</label>
					<input type="password" bind:value={confirmPw} class="w-full h-10 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100" />
				</div>
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
		</Card>
	{/if}
</div>
