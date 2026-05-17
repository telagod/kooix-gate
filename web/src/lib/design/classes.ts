import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

export const text = {
	primary: 'text-zinc-900 dark:text-zinc-100',
	secondary: 'text-zinc-600 dark:text-zinc-300',
	muted: 'text-zinc-500 dark:text-zinc-400',
	disabled: 'text-zinc-400 dark:text-zinc-500',
	inverted: 'text-white dark:text-zinc-900',
	danger: 'text-red-600 dark:text-red-400',
	warning: 'text-amber-700 dark:text-amber-400',
	success: 'text-green-700 dark:text-green-400'
} as const;

export const surface = {
	page: 'bg-zinc-50 dark:bg-zinc-950',
	base: 'bg-white dark:bg-zinc-900',
	raised: 'bg-zinc-50 dark:bg-zinc-800',
	sunken: 'bg-zinc-100 dark:bg-zinc-800/70',
	border: 'border-zinc-200 dark:border-zinc-700',
	borderSubtle: 'border-zinc-100 dark:border-zinc-800',
	divider: 'divide-zinc-200 dark:divide-zinc-700'
} as const;

export const radius = {
	sm: 'rounded-md',
	md: 'rounded-lg',
	lg: 'rounded-xl',
	xl: 'rounded-2xl',
	full: 'rounded-full'
} as const;

export const focusRing =
	'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-900 dark:focus-visible:ring-zinc-300';

export const interactive = {
	subtle: 'transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800',
	surface: 'transition-all hover:border-zinc-300 dark:hover:border-zinc-600 hover:shadow-sm',
	icon: 'transition-colors text-zinc-400 hover:text-zinc-700 dark:text-zinc-500 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800'
} as const;

export type ButtonVariant = 'default' | 'outline' | 'ghost' | 'destructive';
export type ButtonSize = 'default' | 'sm' | 'lg' | 'icon';

export const buttonVariants: Record<ButtonVariant, string> = {
	default:
		'bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200',
	outline:
		'border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:bg-zinc-50 dark:hover:bg-zinc-800 text-zinc-900 dark:text-zinc-100',
	ghost: 'hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-900 dark:text-zinc-100',
	destructive: 'bg-red-600 text-white hover:bg-red-700 dark:hover:bg-red-500'
};

export const buttonSizes: Record<ButtonSize, string> = {
	default: 'h-10 px-4 py-2 text-sm',
	sm: 'h-8 px-3 text-xs',
	lg: 'h-12 px-8 text-base',
	icon: 'h-9 w-9 p-0'
};

export function buttonClass({
	variant = 'default',
	size = 'default',
	class: className
}: {
	variant?: ButtonVariant;
	size?: ButtonSize;
	class?: ClassValue;
} = {}) {
	return cn(
		'inline-flex items-center justify-center gap-2 rounded-md font-medium transition-colors disabled:pointer-events-none disabled:opacity-50',
		focusRing,
		buttonVariants[variant],
		buttonSizes[size],
		className
	);
}

export type CardVariant = 'default' | 'raised' | 'subtle' | 'success' | 'warning' | 'danger';
export type CardPadding = 'none' | 'sm' | 'md' | 'lg';

export const cardVariants: Record<CardVariant, string> = {
	default: 'border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-sm',
	raised: 'border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-md',
	subtle: 'border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/60 shadow-sm',
	success: 'border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/20 shadow-sm',
	warning: 'border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20 shadow-sm',
	danger: 'border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 shadow-sm'
};

export const cardPadding: Record<CardPadding, string> = {
	none: '',
	sm: 'p-4',
	md: 'p-5',
	lg: 'p-6'
};

export function cardClass({
	variant = 'default',
	padding = 'none',
	interactive: isInteractive = false,
	class: className
}: {
	variant?: CardVariant;
	padding?: CardPadding;
	interactive?: boolean;
	class?: ClassValue;
} = {}) {
	return cn('rounded-lg', cardVariants[variant], cardPadding[padding], isInteractive && interactive.surface, className);
}

export type ControlSize = 'default' | 'sm';

export function controlClass({
	size = 'default',
	invalid = false,
	class: className
}: {
	size?: ControlSize;
	invalid?: boolean;
	class?: ClassValue;
} = {}) {
	return cn(
		'flex w-full rounded-md border bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 disabled:cursor-not-allowed disabled:opacity-50',
		size === 'sm' ? 'h-9 px-2.5 py-1.5 text-sm' : 'h-10 px-3 py-2 text-sm',
		invalid ? 'border-red-300 dark:border-red-700' : 'border-zinc-200 dark:border-zinc-700',
		'focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300',
		className
	);
}

export function textareaClass(options: Parameters<typeof controlClass>[0] = {}) {
	return cn(controlClass(options), 'min-h-24 resize-y');
}

export type BadgeVariant = 'default' | 'success' | 'warning' | 'danger' | 'admin';

export const badgeVariants: Record<BadgeVariant, string> = {
	default: 'bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300 ring-zinc-200 dark:ring-zinc-700',
	success: 'bg-green-50 text-green-700 dark:bg-green-900/30 dark:text-green-400 ring-green-200 dark:ring-green-800',
	warning: 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400 ring-amber-200 dark:ring-amber-800',
	danger: 'bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-400 ring-red-200 dark:ring-red-800',
	admin: 'bg-amber-500/20 text-amber-700 dark:text-amber-400 ring-amber-500/20'
};

export function badgeClass({
	variant = 'default',
	class: className
}: {
	variant?: BadgeVariant;
	class?: ClassValue;
} = {}) {
	return cn('inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-xs font-medium ring-1 ring-inset', badgeVariants[variant], className);
}

export type AlertVariant = 'info' | 'success' | 'warning' | 'danger';

export const alertVariants: Record<AlertVariant, string> = {
	info: 'border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/60 text-zinc-700 dark:text-zinc-300',
	success: 'border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/20 text-green-800 dark:text-green-300',
	warning: 'border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20 text-amber-800 dark:text-amber-300',
	danger: 'border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300'
};

export function alertClass({
	variant = 'info',
	class: className
}: {
	variant?: AlertVariant;
	class?: ClassValue;
} = {}) {
	return cn('rounded-lg border px-4 py-3 text-sm', alertVariants[variant], className);
}

export const skeletonClass = 'animate-pulse rounded-lg bg-zinc-200 dark:bg-zinc-700';

export const pageTemplate = {
	shell: 'mx-auto w-full p-6',
	max: {
		narrow: 'max-w-3xl',
		default: 'max-w-5xl',
		wide: 'max-w-7xl',
		full: 'max-w-none'
	},
	header: 'mb-6 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between',
	titleWrap: 'min-w-0',
	eyebrow: 'mb-1 text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400',
	title: 'text-2xl font-bold tracking-tight text-zinc-900 dark:text-zinc-100',
	description: 'mt-1 text-sm text-zinc-600 dark:text-zinc-300',
	actions: 'flex shrink-0 flex-wrap items-center gap-2',
	icon: 'flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
} as const;

export const dataTemplate = {
	toolbar: 'mb-3 space-y-3',
	toolbarRow: 'flex flex-wrap items-center gap-3',
	toolbarSearch: 'relative min-w-[200px] max-w-sm flex-1',
	toolbarFilters: 'flex flex-wrap items-center gap-3',
	toolbarActions: 'flex flex-wrap items-center gap-2',
	toolbarBadges: 'flex flex-wrap gap-1.5',
	filterPanel:
		'mb-4 space-y-4 rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40',
	tableWrap:
		'mb-4 overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm dark:border-zinc-700 dark:bg-zinc-900',
	table: 'w-full text-sm',
	head: 'border-b border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800/60',
	body: 'divide-y divide-zinc-100 dark:divide-zinc-800',
	foot: 'border-t border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800/60',
	th: 'px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400',
	td: 'px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400',
	tdStrong: 'px-4 py-3 text-xs text-zinc-900 dark:text-zinc-100',
	tdMono: 'px-4 py-3 font-mono text-xs text-zinc-600 dark:text-zinc-400',
	tdMonoStrong: 'px-4 py-3 font-mono text-xs text-zinc-900 dark:text-zinc-100',
	row: 'transition-colors hover:bg-zinc-50 dark:hover:bg-zinc-800/30',
	rowInteractive: 'cursor-pointer transition-colors hover:bg-zinc-50 dark:hover:bg-zinc-800/30',
	rowSelected: 'bg-zinc-50 dark:bg-zinc-800/50',
	emptyCell: 'px-4 py-8 text-center text-sm text-zinc-500 dark:text-zinc-400',
	pagination: 'flex items-center justify-between text-sm text-zinc-600 dark:text-zinc-400'
} as const;

export const authTemplate = {
	shell: 'min-h-screen bg-zinc-50 dark:bg-zinc-950 flex items-center justify-center p-4',
	card: 'w-full p-8',
	max: {
		sm: 'max-w-sm',
		md: 'max-w-md',
		lg: 'max-w-lg'
	},
	header: 'mb-8 text-center',
	title: 'text-2xl font-bold text-zinc-900 dark:text-zinc-100',
	description: 'mt-1 text-sm text-zinc-600 dark:text-zinc-300',
	themeToggle:
		'fixed right-4 top-4 rounded-lg p-2 text-zinc-400 transition-colors hover:bg-zinc-200/60 hover:text-zinc-600 dark:hover:bg-zinc-800/60 dark:hover:text-zinc-200'
} as const;
