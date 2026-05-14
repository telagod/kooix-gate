<script lang="ts">
	import { goto } from '$app/navigation';
	import { postSetup, login } from '$lib/api.js';
	import { saveTokens, currentUser } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let step = $state(1);
	let email = $state('');
	let password = $state('');
	let passwordConfirm = $state('');
	let orgName = $state('default');
	let orgSlug = $state('default');
	let projectName = $state('default');
	let projectSlug = $state('default');
	let error = $state('');
	let loading = $state(false);
	let done = $state(false);
	let resultEmail = $state('');

	function validateStep1(): boolean {
		if (!email.trim()) { error = '请输入管理员邮箱'; return false; }
		if (password.length < 8) { error = '密码至少 8 个字符'; return false; }
		if (password !== passwordConfirm) { error = '两次密码不一致'; return false; }
		error = '';
		return true;
	}

	function nextStep() {
		if (step === 1 && validateStep1()) {
			step = 2;
		}
	}

	function prevStep() {
		if (step === 2) step = 1;
		error = '';
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (step === 1) { nextStep(); return; }

		loading = true;
		error = '';
		try {
			await postSetup({
				email: email.trim(),
				password,
				org_name: orgName.trim() || 'default',
				org_slug: orgSlug.trim() || 'default',
				project_name: projectName.trim() || 'default',
				project_slug: projectSlug.trim() || 'default'
			});
			resultEmail = email.trim();
			done = true;
		} catch (err: any) {
			error = err?.message ?? '初始化失败';
		} finally {
			loading = false;
		}
	}

	async function handleLogin() {
		loading = true;
		error = '';
		try {
			const result = await login(resultEmail, password);
			saveTokens(result.access_token, result.refresh_token);
			currentUser.set(result.user);
			await goto('/orgs');
		} catch (err: any) {
			error = err?.message ?? '自动登录失败，请手动登录';
			await goto('/login');
		} finally {
			loading = false;
		}
	}
</script>

<div class="min-h-screen bg-zinc-50 dark:bg-zinc-950 flex items-center justify-center p-4">
	<Card class="w-full max-w-lg p-8">
		{#if done}
			<div class="text-center">
				<div class="w-16 h-16 bg-green-100 dark:bg-green-900/30 rounded-full flex items-center justify-center mx-auto mb-4">
					<span class="text-green-600 dark:text-green-400 text-2xl font-bold">✓</span>
				</div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 mb-2">初始化完成</h1>
				<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6">管理员账号 <span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{resultEmail}</span> 已创建</p>
				<Button class="w-full" onclick={handleLogin} disabled={loading}>
					{loading ? '登录中...' : '进入控制台'}
				</Button>
				{#if error}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2 mt-4">{error}</p>
				{/if}
			</div>
		{:else}
			<div class="mb-6">
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">Kooix Gate 初始化</h1>
				<p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">首次使用，请创建管理员账号和默认组织</p>
				<div class="flex gap-2 mt-4">
					<div class="flex-1 h-1 rounded-full {step >= 1 ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-200 dark:bg-zinc-700'}"></div>
					<div class="flex-1 h-1 rounded-full {step >= 2 ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-200 dark:bg-zinc-700'}"></div>
				</div>
				<p class="text-xs text-zinc-400 dark:text-zinc-500 mt-2">步骤 {step} / 2</p>
			</div>

			<form onsubmit={handleSubmit} class="space-y-4">
				{#if step === 1}
					<div>
						<label for="email" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">管理员邮箱</label>
						<Input id="email" type="email" placeholder="admin@example.com" bind:value={email} disabled={loading} autocomplete="email" />
					</div>
					<div>
						<label for="password" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">密码（至少 8 位）</label>
						<Input id="password" type="password" placeholder="••••••••" bind:value={password} disabled={loading} autocomplete="new-password" />
					</div>
					<div>
						<label for="passwordConfirm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">确认密码</label>
						<Input id="passwordConfirm" type="password" placeholder="••••••••" bind:value={passwordConfirm} disabled={loading} autocomplete="new-password" />
					</div>
				{:else}
					<div class="grid grid-cols-2 gap-3">
						<div>
							<label for="orgName" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">组织名称</label>
							<Input id="orgName" placeholder="My Org" bind:value={orgName} disabled={loading} />
						</div>
						<div>
							<label for="orgSlug" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">组织 Slug</label>
							<Input id="orgSlug" placeholder="my-org" bind:value={orgSlug} disabled={loading} />
						</div>
					</div>
					<div class="grid grid-cols-2 gap-3">
						<div>
							<label for="projName" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">项目名称</label>
							<Input id="projName" placeholder="My Project" bind:value={projectName} disabled={loading} />
						</div>
						<div>
							<label for="projSlug" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">项目 Slug</label>
							<Input id="projSlug" placeholder="my-project" bind:value={projectSlug} disabled={loading} />
						</div>
					</div>
					<div class="bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg px-4 py-3">
						<p class="text-xs text-zinc-500 dark:text-zinc-400">组织和项目可在初始化后随时修改，这里用默认值即可快速开始。</p>
					</div>
				{/if}

				{#if error}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{error}</p>
				{/if}

				<div class="flex gap-2">
					{#if step === 2}
						<Button variant="outline" type="button" onclick={prevStep} disabled={loading}>上一步</Button>
					{/if}
					<Button type="submit" disabled={loading} class="flex-1">
						{#if step === 1}
							下一步
						{:else}
							{loading ? '初始化中...' : '完成初始化'}
						{/if}
					</Button>
				</div>
			</form>
		{/if}
	</Card>
</div>
