<script lang="ts">
	// 0.4.13：从 quotas/+page.svelte 抽出的 form modal。
	import { Badge, Button, Card, Input, Select } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';

	type SelectOption = { value: string; label: string };

	interface Props {
		showForm: boolean;
		formScopeKind: string;
		formScopeId: string;
		formDimension: string;
		formLimitValue: string;
		formModelFilter: string;
		formWindowSeconds: string;
		formMode: 'enforce' | 'dry_run';
		formError: string;
		submitting: boolean;
		orgId: string;
		scopeOptions: SelectOption[];
		dimensionOptions: SelectOption[];
		modeOptions: SelectOption[];
		text: { primary: string; secondary: string; muted: string };
		onClose: () => void;
		onSubmit: (e: SubmitEvent) => void | Promise<void>;
	}

	let {
		showForm = $bindable(),
		formScopeKind = $bindable(),
		formScopeId = $bindable(),
		formDimension = $bindable(),
		formLimitValue = $bindable(),
		formModelFilter = $bindable(),
		formWindowSeconds = $bindable(),
		formMode = $bindable(),
		formError,
		submitting,
		orgId,
		scopeOptions,
		dimensionOptions,
		modeOptions,
		text,
		onClose,
		onSubmit,
	}: Props = $props();
</script>

{#if showForm}
	<ModalFrame close={onClose} panelClass="w-full max-w-2xl">
		<Card padding="lg" class="max-h-[90vh] overflow-y-auto">
			<div class="mb-5 flex items-start justify-between gap-3">
				<div>
					<p class="text-xs font-semibold uppercase tracking-widest {text.muted}">Policy Rule</p>
					<h3 class="mt-1 text-lg font-semibold {text.primary}">添加配额策略</h3>
					<p class="mt-1 text-sm {text.secondary}">user × model / api_key × model 均可用 model_filter 精确收束。</p>
				</div>
				<Badge variant={formMode === 'dry_run' ? 'warning' : 'default'}>{formMode}</Badge>
			</div>

			<form onsubmit={onSubmit} class="space-y-4">
				<div class="grid gap-3 md:grid-cols-3">
					<div>
						<label for="q-scope" class="mb-1 block text-sm font-medium {text.secondary}">作用域</label>
						<Select id="q-scope" bind:value={formScopeKind} options={scopeOptions} disabled={submitting} />
					</div>
					<div>
						<label for="q-dim" class="mb-1 block text-sm font-medium {text.secondary}">维度</label>
						<Select id="q-dim" bind:value={formDimension} options={dimensionOptions} disabled={submitting} />
					</div>
					<div>
						<label for="q-mode" class="mb-1 block text-sm font-medium {text.secondary}">模式</label>
						<Select id="q-mode" bind:value={formMode} options={modeOptions} disabled={submitting} />
					</div>
				</div>

				<div>
					<label for="q-scope-id" class="mb-1 block text-sm font-medium {text.secondary}">Scope ID</label>
					<Input id="q-scope-id" placeholder={orgId} bind:value={formScopeId} disabled={submitting} />
					<p class="mt-1 text-xs {text.muted}">Org 填当前 Org ID；Project / API Key / User 填对应 UUID 或 typed ID。</p>
				</div>

				<div class="grid gap-3 md:grid-cols-3">
					<div>
						<label for="q-limit" class="mb-1 block text-sm font-medium {text.secondary}">限额</label>
						<Input id="q-limit" type="number" step="any" min="0" bind:value={formLimitValue} disabled={submitting} />
					</div>
					<div>
						<label for="q-model" class="mb-1 block text-sm font-medium {text.secondary}">Model filter</label>
						<Input id="q-model" placeholder="* 或 gpt-4o-*" bind:value={formModelFilter} disabled={submitting} />
					</div>
					<div>
						<label for="q-window" class="mb-1 block text-sm font-medium {text.secondary}">窗口(秒)</label>
						<Input id="q-window" type="number" min="1" bind:value={formWindowSeconds} disabled={submitting} />
					</div>
				</div>

				{#if formError}
					<p class="rounded-md bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-900/20 dark:text-red-400">{formError}</p>
				{/if}

				<div class="flex justify-end gap-2 pt-2">
					<Button variant="outline" type="button" onclick={onClose} disabled={submitting}>取消</Button>
					<Button type="submit" disabled={submitting}>{submitting ? '保存中...' : '保存'}</Button>
				</div>
			</form>
		</Card>
	</ModalFrame>
{/if}
