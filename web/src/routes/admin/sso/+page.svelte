<script lang="ts">
	import { onMount } from 'svelte';
	import {
		createIdentityProvider,
		deleteIdentityProvider,
		discoverIdentityProvider,
		listIdentityProviders,
		updateIdentityProvider
	} from '$lib/api.js';
	import type { IdentityProvider } from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, Input, Select, Skeleton, Textarea } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate, text } from '$lib/design';
	import { KeyRound, Plus, RefreshCcw, Search, ShieldCheck, Trash2, Save, X, Compass } from 'lucide-svelte';

	type FormState = {
		id: string | null;
		name: string;
		slug: string;
		issuer: string;
		client_id: string;
		client_secret: string;
		scopesText: string;
		email_claim: string;
		name_claim: string;
		subject_claim: string;
		auto_create_users: boolean;
		auto_join_org_role: string;
		emailDomainsText: string;
		enabled: boolean;
		allow_relative: boolean;
		allowedOriginsText: string;
	};

	let providers = $state<IdentityProvider[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let saving = $state(false);
	let deletingId = $state('');
	let discovering = $state(false);
	let error = $state('');
	let success = $state('');
	let search = $state('');
	let mode = $state<'create' | 'edit'>('create');

	const roleOptions = [
		{ value: '', label: '不自动加入 Org' },
		{ value: 'member', label: 'member' },
		{ value: 'billing_viewer', label: 'billing_viewer' },
		{ value: 'admin', label: 'admin' },
		{ value: 'owner', label: 'owner' }
	];

	let form = $state<FormState>(emptyForm());

	let enabledCount = $derived(providers.filter((p) => p.enabled).length);
	let domainCount = $derived(new Set(providers.flatMap((p) => p.email_domain_allowlist)).size);
	let filteredProviders = $derived.by(() => {
		const q = search.trim().toLowerCase();
		if (!q) return providers;
		return providers.filter((p) =>
			[p.name, p.slug, p.issuer, p.client_id, p.email_domain_allowlist.join(' ')]
				.join(' ')
				.toLowerCase()
				.includes(q)
		);
	});

	onMount(loadProviders);

	function emptyForm(): FormState {
		return {
			id: null,
			name: '',
			slug: '',
			issuer: '',
			client_id: '',
			client_secret: '',
			scopesText: 'openid email profile',
			email_claim: 'email',
			name_claim: 'name',
			subject_claim: 'sub',
			auto_create_users: true,
			auto_join_org_role: '',
			emailDomainsText: '',
			enabled: true,
			allow_relative: true,
			allowedOriginsText: ''
		};
	}

	async function loadProviders() {
		refreshing = true;
		error = '';
		try {
			providers = await listIdentityProviders();
		} catch (err: any) {
			error = err?.message ?? '加载 SSO Provider 失败';
		} finally {
			loading = false;
			refreshing = false;
		}
	}

	function editProvider(provider: IdentityProvider) {
		mode = 'edit';
		form = {
			id: provider.id,
			name: provider.name,
			slug: provider.slug,
			issuer: provider.issuer,
			client_id: provider.client_id,
			client_secret: '',
			scopesText: provider.scopes.join(' '),
			email_claim: provider.email_claim,
			name_claim: provider.name_claim,
			subject_claim: provider.subject_claim,
			auto_create_users: provider.auto_create_users,
			auto_join_org_role: provider.auto_join_org_role ?? '',
			emailDomainsText: provider.email_domain_allowlist.join('\n'),
			enabled: provider.enabled,
			allow_relative: provider.redirect_policy.allow_relative,
			allowedOriginsText: provider.redirect_policy.allowed_origins.join('\n')
		};
		success = '';
		error = '';
	}

	function resetCreate() {
		mode = 'create';
		form = emptyForm();
		error = '';
		success = '';
	}

	async function submitForm() {
		saving = true;
		error = '';
		success = '';
		try {
			const payload = buildPayload();
			if (mode === 'create') {
				const created = await createIdentityProvider({ ...payload, client_secret: form.client_secret });
				providers = [created, ...providers];
				success = `已创建 ${created.name}`;
				editProvider(created);
			} else if (form.id) {
				const updatePayload: any = payload;
				if (form.client_secret.trim()) updatePayload.client_secret = form.client_secret.trim();
				const updated = await updateIdentityProvider(form.id, updatePayload);
				providers = providers.map((p) => (p.id === updated.id ? updated : p));
				success = `已更新 ${updated.name}`;
				editProvider(updated);
			}
		} catch (err: any) {
			error = err?.message ?? '保存失败';
		} finally {
			saving = false;
		}
	}

	function buildPayload() {
		const scopes = splitList(form.scopesText, /[\s,]+/);
		const emailDomains = splitList(form.emailDomainsText, /[\s,]+/).map((d) => d.replace(/^@/, '').toLowerCase());
		const allowedOrigins = splitList(form.allowedOriginsText, /[\n,]+/);
		return {
			name: form.name.trim(),
			slug: form.slug.trim().toLowerCase(),
			issuer: form.issuer.trim(),
			client_id: form.client_id.trim(),
			scopes,
			email_claim: form.email_claim.trim() || 'email',
			name_claim: form.name_claim.trim() || 'name',
			subject_claim: form.subject_claim.trim() || 'sub',
			auto_create_users: form.auto_create_users,
			auto_join_org_role: form.auto_join_org_role || null,
			email_domain_allowlist: emailDomains,
			enabled: form.enabled,
			redirect_policy: {
				allow_relative: form.allow_relative,
				allowed_origins: allowedOrigins
			}
		};
	}

	function splitList(value: string, pattern: RegExp): string[] {
		return value
			.split(pattern)
			.map((v) => v.trim())
			.filter(Boolean);
	}

	async function discover() {
		if (!form.issuer.trim()) {
			error = '先填写 issuer';
			return;
		}
		discovering = true;
		error = '';
		success = '';
		try {
			const meta = await discoverIdentityProvider(form.issuer.trim());
			form.issuer = meta.issuer;
			if (meta.scopes_supported.length) {
				const preferred = ['openid', 'email', 'profile'].filter((scope) => meta.scopes_supported.includes(scope));
				form.scopesText = (preferred.length ? preferred : meta.scopes_supported.slice(0, 6)).join(' ');
			}
			success = 'OIDC discovery 通过：authorization_endpoint / token_endpoint / jwks_uri 已验证';
		} catch (err: any) {
			error = err?.message ?? 'Discovery 失败';
		} finally {
			discovering = false;
		}
	}

	async function toggleProvider(provider: IdentityProvider) {
		try {
			const updated = await updateIdentityProvider(provider.id, { enabled: !provider.enabled });
			providers = providers.map((p) => (p.id === updated.id ? updated : p));
		} catch (err: any) {
			error = err?.message ?? '状态切换失败';
		}
	}

	async function removeProvider(provider: IdentityProvider) {
		if (!confirm(`删除 SSO Provider：${provider.name}？`)) return;
		deletingId = provider.id;
		error = '';
		try {
			await deleteIdentityProvider(provider.id);
			providers = providers.filter((p) => p.id !== provider.id);
			if (form.id === provider.id) resetCreate();
			success = `已删除 ${provider.name}`;
		} catch (err: any) {
			error = err?.message ?? '删除失败';
		} finally {
			deletingId = '';
		}
	}

	function providerBadgeVariant(provider: IdentityProvider) {
		return provider.enabled ? 'success' : 'default';
	}
</script>

<PageShell title="SSO Provider" description="管理 OIDC Provider、邮箱 allowlist、auto-join 与 redirect policy" icon={KeyRound} max="wide">
	{#snippet actions()}
		<Button variant="outline" onclick={loadProviders} disabled={refreshing || loading}>
			<RefreshCcw size={14} class={refreshing ? 'animate-spin' : ''} />
			刷新
		</Button>
		<Button onclick={resetCreate}>
			<Plus size={14} />
			新建
		</Button>
	{/snippet}

	{#if error}<Alert variant="danger" class="mb-4">{error}</Alert>{/if}
	{#if success}<Alert variant="success" class="mb-4">{success}</Alert>{/if}

	<div class="mb-4 grid gap-3 md:grid-cols-3">
		<Card padding="md">
			<p class="text-xs uppercase tracking-wider {text.muted}">Providers</p>
			<p class="mt-1 text-2xl font-semibold {text.primary}">{providers.length}</p>
		</Card>
		<Card padding="md" variant="success">
			<p class="text-xs uppercase tracking-wider {text.success}">Enabled</p>
			<p class="mt-1 text-2xl font-semibold {text.primary}">{enabledCount}</p>
		</Card>
		<Card padding="md">
			<p class="text-xs uppercase tracking-wider {text.muted}">Allowlist Domains</p>
			<p class="mt-1 text-2xl font-semibold {text.primary}">{domainCount}</p>
		</Card>
	</div>

	<div class="grid gap-4 xl:grid-cols-[minmax(0,1fr)_430px]">
		<section class="space-y-4">
			<Card padding="sm">
				<div class="relative">
					<Search size={14} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
					<Input class="pl-9" placeholder="搜索 name / slug / issuer / domain" bind:value={search} />
				</div>
			</Card>

			{#if loading}
				<div class="space-y-2">
					{#each Array(5) as _}<Skeleton class="h-14" />{/each}
				</div>
			{:else if providers.length === 0}
				<StatePanel title="暂无 SSO Provider" description="创建 OIDC Provider 后，登录页会自动显示可用 SSO 入口。" icon={ShieldCheck} />
			{:else}
				<DataTable isEmpty={filteredProviders.length === 0} emptyColspan={7}>
					{#snippet head()}
						<tr>
							<th class={dataTemplate.th}>Provider</th>
							<th class={dataTemplate.th}>Issuer</th>
							<th class={dataTemplate.th}>Allowlist</th>
							<th class={dataTemplate.th}>Redirect Policy</th>
							<th class={dataTemplate.th}>JIT</th>
							<th class={dataTemplate.th}>状态</th>
							<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
						</tr>
					{/snippet}

					{#snippet empty()}
						无匹配 Provider
					{/snippet}

					{#each filteredProviders as provider}
						<tr class={cn(dataTemplate.rowInteractive, form.id === provider.id && dataTemplate.rowSelected)} onclick={() => editProvider(provider)}>
							<td class={dataTemplate.tdStrong}>
								<div class="font-medium">{provider.name}</div>
								<div class="font-mono text-[11px] {text.muted}">{provider.slug}</div>
							</td>
							<td class={cn(dataTemplate.tdMono, 'max-w-[220px] truncate')}>{provider.issuer}</td>
							<td class={dataTemplate.td}>
								{#if provider.email_domain_allowlist.length === 0}
									<span class={text.muted}>不限</span>
								{:else}
									<div class="flex flex-wrap gap-1">
										{#each provider.email_domain_allowlist.slice(0, 2) as domain}<Badge>{domain}</Badge>{/each}
										{#if provider.email_domain_allowlist.length > 2}<Badge>+{provider.email_domain_allowlist.length - 2}</Badge>{/if}
									</div>
								{/if}
							</td>
							<td class={dataTemplate.td}>
								<div>{provider.redirect_policy.allow_relative ? 'relative allowed' : 'relative denied'}</div>
								<div class="text-[11px] {text.muted}">{provider.redirect_policy.allowed_origins.length} origins</div>
							</td>
							<td class={dataTemplate.td}>{provider.auto_create_users ? 'auto-create' : 'manual'}{provider.auto_join_org_role ? ` · ${provider.auto_join_org_role}` : ''}</td>
							<td class={dataTemplate.td}><Badge variant={providerBadgeVariant(provider)}>{provider.enabled ? 'Enabled' : 'Disabled'}</Badge></td>
							<td class="px-4 py-3 text-right" onclick={(e) => e.stopPropagation()}>
								<div class="flex justify-end gap-1">
									<Button size="sm" variant="outline" onclick={() => toggleProvider(provider)}>{provider.enabled ? '停用' : '启用'}</Button>
									<button class="rounded p-2 text-zinc-400 transition-colors hover:text-red-600 disabled:opacity-50" disabled={deletingId === provider.id} onclick={() => removeProvider(provider)} aria-label="删除 Provider">
										<Trash2 size={14} />
									</button>
								</div>
							</td>
						</tr>
					{/each}
				</DataTable>
			{/if}
		</section>

		<Card padding="md" class="h-fit">
			<div class="mb-4 flex items-start justify-between gap-3">
				<div>
					<p class="text-sm font-semibold {text.primary}">{mode === 'create' ? '新建 Provider' : '编辑 Provider'}</p>
					<p class="text-xs {text.muted}">client_secret 只提交一次；编辑时留空表示不轮换。</p>
				</div>
				{#if mode === 'edit'}
					<Button size="sm" variant="ghost" onclick={resetCreate}><X size={14} />取消编辑</Button>
				{/if}
			</div>

			<form class="space-y-4" onsubmit={(e) => { e.preventDefault(); submitForm(); }}>
				<div class="grid gap-3 sm:grid-cols-2">
					<Field label="名称" for="idp-name" required><Input id="idp-name" bind:value={form.name} placeholder="Google Workspace" /></Field>
					<Field label="Slug" for="idp-slug" required hint="用于 /auth/sso/:slug/start"><Input id="idp-slug" bind:value={form.slug} placeholder="google" /></Field>
				</div>

				<Field label="Issuer" for="idp-issuer" required hint="必须支持 .well-known/openid-configuration；生产要求 HTTPS。">
					<div class="flex gap-2">
						<Input id="idp-issuer" bind:value={form.issuer} placeholder="https://accounts.google.com" />
						<Button type="button" variant="outline" disabled={discovering} onclick={discover}><Compass size={14} />{discovering ? '检测中' : 'Discover'}</Button>
					</div>
				</Field>

				<div class="grid gap-3 sm:grid-cols-2">
					<Field label="Client ID" for="idp-client-id" required><Input id="idp-client-id" bind:value={form.client_id} /></Field>
					<Field label={mode === 'create' ? 'Client Secret' : 'Client Secret (可选轮换)'} for="idp-secret" required={mode === 'create'}>
						<Input id="idp-secret" type="password" bind:value={form.client_secret} autocomplete="new-password" />
					</Field>
				</div>

				<Field label="Scopes" for="idp-scopes" hint="空格或逗号分隔"><Input id="idp-scopes" bind:value={form.scopesText} /></Field>

				<div class="grid gap-3 sm:grid-cols-3">
					<Field label="Email claim" for="idp-email-claim"><Input id="idp-email-claim" bind:value={form.email_claim} /></Field>
					<Field label="Name claim" for="idp-name-claim"><Input id="idp-name-claim" bind:value={form.name_claim} /></Field>
					<Field label="Subject claim" for="idp-sub-claim"><Input id="idp-sub-claim" bind:value={form.subject_claim} /></Field>
				</div>

				<Field label="Email domain allowlist" for="idp-domains" hint="每行或逗号分隔；留空则不限。">
					<Textarea id="idp-domains" rows={3} bind:value={form.emailDomainsText} placeholder="example.com&#10;corp.example.com" />
				</Field>

				<div class="grid gap-3 sm:grid-cols-2">
					<label class="flex items-center gap-2 rounded-md border border-zinc-200 p-3 text-sm dark:border-zinc-700">
						<input type="checkbox" bind:checked={form.auto_create_users} class="h-4 w-4 accent-zinc-900 dark:accent-zinc-100" />
						<span class={text.secondary}>自动创建用户</span>
					</label>
					<Field label="Auto-join role" for="idp-role"><Select id="idp-role" bind:value={form.auto_join_org_role} options={roleOptions} /></Field>
				</div>

				<div class="rounded-lg border border-zinc-200 p-3 dark:border-zinc-700">
					<div class="mb-3 flex items-center gap-2"><ShieldCheck size={15} class={text.secondary} /><p class="text-sm font-medium {text.primary}">Redirect policy</p></div>
					<label class="mb-3 flex items-center gap-2 text-sm {text.secondary}">
						<input type="checkbox" bind:checked={form.allow_relative} class="h-4 w-4 accent-zinc-900 dark:accent-zinc-100" />
						允许相对路径 redirect_to（如 /orgs）
					</label>
					<Field label="Allowed origins" for="idp-origins" hint="绝对跳转只允许这些 origin；每行一个。">
						<Textarea id="idp-origins" rows={3} bind:value={form.allowedOriginsText} placeholder="https://console.example.com" />
					</Field>
				</div>

				<label class="flex items-center gap-2 text-sm {text.secondary}">
					<input type="checkbox" bind:checked={form.enabled} class="h-4 w-4 accent-zinc-900 dark:accent-zinc-100" />
					启用 Provider
				</label>

				<Button type="submit" disabled={saving || !form.name.trim() || !form.slug.trim() || !form.issuer.trim() || !form.client_id.trim() || (mode === 'create' && !form.client_secret.trim())} class="w-full">
					<Save size={14} />
					{saving ? '保存中...' : mode === 'create' ? '创建 Provider' : '保存 Provider'}
				</Button>
			</form>
		</Card>
	</div>
</PageShell>
