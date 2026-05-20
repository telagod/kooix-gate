<script lang="ts">
	type MarkedApi = typeof import('marked').marked;
	type HighlightApi = typeof import('highlight.js/lib/core').default;
	type LanguageFn = Awaited<typeof import('highlight.js/lib/languages/typescript')>['default'];

	let { content = '', streaming = false }: { content: string; streaming?: boolean } = $props();

	const languageAliases: Record<string, string> = {
		js: 'javascript',
		ts: 'typescript',
		sh: 'bash',
		console: 'shell',
		html: 'xml',
		svelte: 'xml',
		md: 'markdown',
		yml: 'yaml',
		text: 'plaintext',
		txt: 'plaintext'
	};

	type MarkdownRuntime = {
		marked: MarkedApi;
		hljs: HighlightApi;
	};

	let runtimePromise: Promise<MarkdownRuntime> | null = null;
	let html = $state('');
	let renderGeneration = 0;

	function escapeHtml(value: string) {
		return value
			.replace(/&/g, '&amp;')
			.replace(/</g, '&lt;')
			.replace(/>/g, '&gt;')
			.replace(/"/g, '&quot;')
			.replace(/'/g, '&#39;');
	}

	function plainTextFallback(value: string) {
		return escapeHtml(value).replace(/\n/g, '<br>');
	}

	function loadMarkdownRuntime() {
		runtimePromise ??= Promise.all([
			import('marked'),
			import('highlight.js/lib/core'),
			import('highlight.js/lib/languages/bash'),
			import('highlight.js/lib/languages/css'),
			import('highlight.js/lib/languages/diff'),
			import('highlight.js/lib/languages/javascript'),
			import('highlight.js/lib/languages/json'),
			import('highlight.js/lib/languages/markdown'),
			import('highlight.js/lib/languages/plaintext'),
			import('highlight.js/lib/languages/python'),
			import('highlight.js/lib/languages/rust'),
			import('highlight.js/lib/languages/shell'),
			import('highlight.js/lib/languages/sql'),
			import('highlight.js/lib/languages/typescript'),
			import('highlight.js/lib/languages/xml'),
			import('highlight.js/lib/languages/yaml')
		]).then(
			([
				{ marked },
				{ default: hljs },
				{ default: bash },
				{ default: css },
				{ default: diff },
				{ default: javascript },
				{ default: json },
				{ default: markdown },
				{ default: plaintext },
				{ default: python },
				{ default: rust },
				{ default: shell },
				{ default: sql },
				{ default: typescript },
				{ default: xml },
				{ default: yaml }
			]) => {
				const languages: Record<string, LanguageFn> = {
					bash,
					css,
					diff,
					javascript,
					json,
					markdown,
					plaintext,
					python,
					rust,
					shell,
					sql,
					typescript,
					xml,
					yaml
				};

				for (const [name, language] of Object.entries(languages)) {
					if (!hljs.getLanguage(name)) hljs.registerLanguage(name, language);
				}

				return { marked, hljs };
			}
		);
		return runtimePromise;
	}

	function renderMarkdown(src: string, isStreaming: boolean, runtime: MarkdownRuntime) {
		function normalizeLanguage(lang?: string) {
			const key = lang?.toLowerCase().trim() ?? '';
			const mapped = languageAliases[key] ?? key;
			return mapped && runtime.hljs.getLanguage(mapped) ? mapped : 'plaintext';
		}

		const renderer = new runtime.marked.Renderer();
		renderer.code = ({ text, lang }: { text: string; lang?: string }) => {
			const language = normalizeLanguage(lang);
			const highlighted = runtime.hljs.highlight(text, { language }).value;
			const escaped = escapeHtml(text);
			return `<div class="code-block group/code relative my-3"><div class="code-head">${language}<button class="copy-btn" data-code="${escaped}">复制</button></div><pre class="code-pre"><code class="hljs language-${language}">${highlighted}</code></pre></div>`;
		};

		let normalized = src;
		if (isStreaming) {
			const fenceCount = (normalized.match(/```/g) || []).length;
			if (fenceCount % 2 !== 0) normalized += '\n```';
		}

		return runtime.marked.parse(normalized, {
			async: false,
			breaks: true,
			gfm: true,
			renderer
		}) as string;
	}

	$effect(() => {
		const src = content;
		const isStreaming = streaming;
		const generation = ++renderGeneration;

		if (!src) {
			html = '';
			return;
		}

		html = plainTextFallback(src);
		loadMarkdownRuntime()
			.then((runtime) => {
				if (generation !== renderGeneration) return;
				try {
					html = renderMarkdown(src, isStreaming, runtime);
				} catch {
					html = plainTextFallback(src);
				}
			})
			.catch(() => {
				if (generation === renderGeneration) html = plainTextFallback(src);
			});
	});

	function handleClick(e: MouseEvent) {
		const btn = (e.target as HTMLElement).closest('.copy-btn') as HTMLElement | null;
		if (!btn) return;
		const code = btn.dataset.code ?? '';
		navigator.clipboard.writeText(code);
		btn.textContent = '已复制';
		setTimeout(() => {
			btn.textContent = '复制';
		}, 2000);
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="md-body" onclick={handleClick}>
	{@html html}
</div>

<style>
	/* Markdown body */
	.md-body { line-height: 1.6; word-wrap: break-word; }
	.md-body :global(p) { margin: 0.5rem 0; }
	.md-body :global(ul), .md-body :global(ol) { margin: 0.5rem 0; padding-left: 1.25rem; }
	.md-body :global(li) { margin: 0.125rem 0; }
	.md-body :global(blockquote) {
		border-left: 2px solid var(--color-zinc-300);
		padding-left: 0.75rem;
		margin: 0.5rem 0;
		color: var(--color-zinc-500);
		font-style: italic;
	}
	.md-body :global(table) { border-collapse: collapse; width: 100%; margin: 0.5rem 0; font-size: 0.875rem; }
	.md-body :global(th), .md-body :global(td) { border: 1px solid var(--color-zinc-200); padding: 0.375rem 0.75rem; }
	.md-body :global(th) { background: var(--color-zinc-50); font-weight: 500; text-align: left; }
	.md-body :global(h1), .md-body :global(h2), .md-body :global(h3) { font-weight: 700; margin-top: 1rem; margin-bottom: 0.5rem; }
	.md-body :global(h1) { font-size: 1.125rem; }
	.md-body :global(h2) { font-size: 1rem; }
	.md-body :global(h3) { font-size: 0.875rem; }
	.md-body :global(hr) { border-color: var(--color-zinc-200); margin: 1rem 0; }
	.md-body :global(a) { text-decoration: underline; }

	/* Inline code */
	.md-body :global(code:not(pre code)) {
		background: var(--color-zinc-100);
		color: var(--color-zinc-800);
		padding: 0.1rem 0.375rem;
		border-radius: 0.25rem;
		font-size: 0.8125rem;
		font-family: var(--font-mono);
	}

	/* Code block */
	.md-body :global(.code-block) { border-radius: 0.5rem; overflow: hidden; }
	.md-body :global(.code-head) {
		display: flex; align-items: center; justify-content: space-between;
		padding: 0.375rem 0.75rem;
		background: #1e1e2e;
		color: #a0a0b0;
		font-size: 0.6875rem;
	}
	.md-body :global(.copy-btn) {
		opacity: 0; transition: opacity 0.15s;
		color: #a0a0b0;
		cursor: pointer; background: none; border: none; font-size: 0.6875rem;
	}
	.md-body :global(.code-block:hover .copy-btn) { opacity: 1; }
	.md-body :global(.copy-btn:hover) { color: #e0e0e0; }
	.md-body :global(.code-pre) {
		margin: 0 !important;
		border-radius: 0 0 0.5rem 0.5rem;
		overflow-x: auto;
		padding: 1rem;
		font-size: 0.8125rem;
		line-height: 1.5;
	}

	/* Dark mode */
	:global(.dark) .md-body :global(code:not(pre code)) { background: var(--color-zinc-800); color: var(--color-zinc-200); }
	:global(.dark) .md-body :global(blockquote) { border-color: var(--color-zinc-600); color: var(--color-zinc-400); }
	:global(.dark) .md-body :global(th) { background: var(--color-zinc-800); }
	:global(.dark) .md-body :global(th), :global(.dark) .md-body :global(td) { border-color: var(--color-zinc-700); }
</style>
