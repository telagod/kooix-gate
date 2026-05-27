<script lang="ts">
	// admin/users/_components/CreateUserForm.svelte — 0.4.65 抽出
	// 父：admin/users/+page.svelte 439-467 行 创建用户 Card
	import { Button, Card, Field, Input, Select } from '$lib/components/ui';
	import { Plus, UserPlus } from 'lucide-svelte';
	import { text } from '$lib/design';

	type CreateForm = { email: string; display_name: string; password: string; status: string };
	type SelectOption = { value: string; label: string };

	type Props = {
		form: CreateForm;
		errors: Record<string, string>;
		creating: boolean;
		statusOptions: SelectOption[];
		onSubmit: () => void;
		onUpdateField: (key: keyof CreateForm, value: string) => void;
	};

	let { form, errors, creating, statusOptions, onSubmit, onUpdateField }: Props = $props();
</script>

<Card padding="md" class="mb-4">
	<div class="mb-4 flex items-center gap-2">
		<UserPlus size={16} class={text.secondary} />
		<div>
			<p class="text-sm font-semibold {text.primary}">创建用户</p>
			<p class="text-xs {text.muted}">密码仅提交给后端做 Argon2id hash，不会回显或入审计明文。</p>
		</div>
	</div>
	<form
		class="grid gap-3 lg:grid-cols-[1.2fr_1fr_1fr_180px_auto]"
		onsubmit={(e) => {
			e.preventDefault();
			onSubmit();
		}}
	>
		<Field label="邮箱" for="create-email" error={errors.email} required>
			<Input
				id="create-email"
				type="email"
				placeholder="user@example.com"
				value={form.email}
				oninput={(e) => onUpdateField('email', (e.currentTarget as HTMLInputElement).value)}
				disabled={creating}
				invalid={!!errors.email}
				autocomplete="off"
			/>
		</Field>
		<Field label="昵称" for="create-name">
			<Input
				id="create-name"
				placeholder="可选"
				value={form.display_name}
				oninput={(e) => onUpdateField('display_name', (e.currentTarget as HTMLInputElement).value)}
				disabled={creating}
			/>
		</Field>
		<Field label="初始密码" for="create-password" error={errors.password} required>
			<Input
				id="create-password"
				type="password"
				placeholder="至少 8 位"
				value={form.password}
				oninput={(e) => onUpdateField('password', (e.currentTarget as HTMLInputElement).value)}
				disabled={creating}
				invalid={!!errors.password}
				autocomplete="new-password"
			/>
		</Field>
		<Field label="状态" for="create-status" error={errors.status}>
			<Select
				id="create-status"
				value={form.status}
				options={statusOptions}
				onchange={(e) => onUpdateField('status', (e.currentTarget as HTMLSelectElement).value)}
				disabled={creating}
			/>
		</Field>
		<div class="flex items-end">
			<Button type="submit" disabled={creating} class="w-full">
				<Plus size={14} />
				创建
			</Button>
		</div>
	</form>
</Card>
