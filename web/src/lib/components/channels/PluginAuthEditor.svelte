<script lang="ts">
	import { Field, Input } from '$lib/components/ui';
	import { PLUGIN_AUTH_STRATEGY_OPTIONS, defaultPluginAuthForm } from '$lib/plugin-presets';
	import type { PluginAuthForm } from '$lib/plugin-presets';

	let {
		form = $bindable<PluginAuthForm>(defaultPluginAuthForm()),
		disabled = false,
		idPrefix = 'plugin-auth'
	}: {
		form?: PluginAuthForm;
		disabled?: boolean;
		idPrefix?: string;
	} = $props();

	const fieldClass = 'rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900/70';
	const inputClass = 'h-9 text-xs';

	function authStrategyDescription(strategy: string): string {
		return PLUGIN_AUTH_STRATEGY_OPTIONS.find(opt => opt.value === strategy)?.description ?? '';
	}
</script>

<div class="mb-3 space-y-3 rounded-lg border border-zinc-200 bg-zinc-100/70 p-3 dark:border-zinc-800 dark:bg-zinc-900/60">
	<div>
		<label for="{idPrefix}-strategy" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Auth Strategy</label>
		<select
			id="{idPrefix}-strategy"
			bind:value={form.strategy}
			{disabled}
			class="w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100"
		>
			{#each PLUGIN_AUTH_STRATEGY_OPTIONS as opt}
				<option value={opt.value}>{opt.label}</option>
			{/each}
		</select>
		<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{authStrategyDescription(form.strategy)}</p>
	</div>

	{#if ['bearer', 'api_key_header', 'api_key_query', 'hmac'].includes(form.strategy)}
		<div class={fieldClass}>
			<Field label="Secret Slot" for="{idPrefix}-secret" hint="来自 channel key alias 或 KOOIX_PLUGIN_SECRET_<SLOT>">
				<Input id="{idPrefix}-secret" size="sm" class={inputClass} bind:value={form.secret_slot} {disabled} />
			</Field>
		</div>
	{/if}

	{#if form.strategy === 'api_key_header'}
		<div class={fieldClass}>
			<Field label="Header Name" for="{idPrefix}-header" required>
				<Input id="{idPrefix}-header" size="sm" class={inputClass} bind:value={form.header_name} {disabled} />
			</Field>
		</div>
	{:else if form.strategy === 'api_key_query'}
		<div class={fieldClass}>
			<Field label="Query Name" for="{idPrefix}-query" required>
				<Input id="{idPrefix}-query" size="sm" class={inputClass} bind:value={form.query_name} {disabled} />
			</Field>
		</div>
	{:else if form.strategy === 'basic'}
		<div class="grid grid-cols-2 gap-3">
			<div class={fieldClass}>
				<Field label="Username Slot" for="{idPrefix}-user" required>
					<Input id="{idPrefix}-user" size="sm" class={inputClass} bind:value={form.username_slot} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Password Slot" for="{idPrefix}-pass">
					<Input id="{idPrefix}-pass" size="sm" class={inputClass} bind:value={form.password_slot} {disabled} />
				</Field>
			</div>
		</div>
	{:else if form.strategy === 'custom_headers'}
		<div class={fieldClass}>
			<label for="{idPrefix}-custom" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Headers JSON</label>
			<textarea
				id="{idPrefix}-custom"
				class="min-h-24 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100"
				bind:value={form.custom_headers}
				{disabled}
			></textarea>
		</div>
	{:else if form.strategy === 'hmac'}
		<div class="grid grid-cols-3 gap-3">
			<div class={fieldClass}>
				<Field label="Signature Header" for="{idPrefix}-hmac-sig" required>
					<Input id="{idPrefix}-hmac-sig" size="sm" class={inputClass} bind:value={form.hmac_signature_header} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Timestamp Header" for="{idPrefix}-hmac-ts" required>
					<Input id="{idPrefix}-hmac-ts" size="sm" class={inputClass} bind:value={form.hmac_timestamp_header} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Nonce Header" for="{idPrefix}-hmac-nonce" required>
					<Input id="{idPrefix}-hmac-nonce" size="sm" class={inputClass} bind:value={form.hmac_nonce_header} {disabled} />
				</Field>
			</div>
		</div>
		<div class={fieldClass}>
			<label for="{idPrefix}-hmac-payload" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Signed Payload</label>
			<textarea
				id="{idPrefix}-hmac-payload"
				class="min-h-24 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100"
				bind:value={form.hmac_signed_payload}
				{disabled}
			></textarea>
		</div>
	{:else if form.strategy === 'aws_sigv4'}
		<div class="grid grid-cols-2 gap-3">
			<div class={fieldClass}>
				<Field label="Service" for="{idPrefix}-aws-service" required>
					<Input id="{idPrefix}-aws-service" size="sm" class={inputClass} bind:value={form.aws_service} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Region" for="{idPrefix}-aws-region" hint="留空则从 host 推断">
					<Input id="{idPrefix}-aws-region" size="sm" class={inputClass} bind:value={form.aws_region} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Access Key Slot" for="{idPrefix}-aws-access">
					<Input id="{idPrefix}-aws-access" size="sm" class={inputClass} bind:value={form.aws_access_key_slot} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Secret Key Slot" for="{idPrefix}-aws-secret" required>
					<Input id="{idPrefix}-aws-secret" size="sm" class={inputClass} bind:value={form.aws_secret_key_slot} {disabled} />
				</Field>
			</div>
		</div>
	{:else if form.strategy === 'oauth_client_credentials'}
		<div class={fieldClass}>
			<Field label="Token URL" for="{idPrefix}-oauth-url" required hint="必须 HTTPS；本地测试允许 localhost">
				<Input id="{idPrefix}-oauth-url" size="sm" class={inputClass} bind:value={form.oauth_token_url} {disabled} />
			</Field>
		</div>
		<div class="grid grid-cols-2 gap-3">
			<div class={fieldClass}>
				<Field label="Client ID Slot" for="{idPrefix}-oauth-id" required>
					<Input id="{idPrefix}-oauth-id" size="sm" class={inputClass} bind:value={form.oauth_client_id_slot} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Client Secret Slot" for="{idPrefix}-oauth-secret" required>
					<Input id="{idPrefix}-oauth-secret" size="sm" class={inputClass} bind:value={form.oauth_client_secret_slot} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Scope" for="{idPrefix}-oauth-scope">
					<Input id="{idPrefix}-oauth-scope" size="sm" class={inputClass} bind:value={form.oauth_scope} {disabled} />
				</Field>
			</div>
			<div class={fieldClass}>
				<Field label="Audience" for="{idPrefix}-oauth-aud">
					<Input id="{idPrefix}-oauth-aud" size="sm" class={inputClass} bind:value={form.oauth_audience} {disabled} />
				</Field>
			</div>
		</div>
	{/if}
</div>
