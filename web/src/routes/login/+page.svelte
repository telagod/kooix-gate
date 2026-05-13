<!-- /login — 邮箱密码登录表单 -->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { login } from '$lib/api.js';
	import { saveTokens, currentUser } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let email = $state('');
	let password = $state('');
	let error = $state('');
	let loading = $state(false);

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (!email || !password) return;

		loading = true;
		error = '';

		try {
			const result = await login(email, password);
			saveTokens(result.access_token, result.refresh_token);
			currentUser.set(result.user);
			await goto('/orgs');
		} catch (err: any) {
			if (err?.code === 'invalid_credentials') {
				error = '邮箱或密码错误';
			} else if (err?.code === 'too_many_failures') {
				error = '登录失败次数过多，请稍后再试';
			} else {
				error = err?.message ?? '登录失败，请重试';
			}
		} finally {
			loading = false;
		}
	}
</script>

<div class="min-h-screen bg-zinc-50 flex items-center justify-center p-4">
	<Card class="w-full max-w-sm p-8">
		<div class="mb-8 text-center">
			<h1 class="text-2xl font-bold text-zinc-900">Kooix Gate</h1>
			<p class="mt-1 text-sm text-zinc-500">登录控制台</p>
		</div>

		<form onsubmit={handleSubmit} class="space-y-4">
			<div>
				<label for="email" class="block text-sm font-medium text-zinc-700 mb-1">邮箱</label>
				<Input
					id="email"
					type="email"
					placeholder="you@example.com"
					bind:value={email}
					disabled={loading}
					autocomplete="email"
				/>
			</div>

			<div>
				<label for="password" class="block text-sm font-medium text-zinc-700 mb-1">密码</label>
				<Input
					id="password"
					type="password"
					placeholder="••••••••"
					bind:value={password}
					disabled={loading}
					autocomplete="current-password"
				/>
			</div>

			{#if error}
				<p class="text-sm text-red-600 bg-red-50 rounded-md px-3 py-2">{error}</p>
			{/if}

			<Button type="submit" disabled={loading} class="w-full">
				{loading ? '登录中...' : '登录'}
			</Button>
		</form>
	</Card>
</div>
