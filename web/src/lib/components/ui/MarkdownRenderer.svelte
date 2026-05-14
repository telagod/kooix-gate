<script lang="ts">
	import { marked } from 'marked';
	import hljs from 'highlight.js';

	let { content = '', streaming = false }: { content: string; streaming?: boolean } = $props();

	const renderer = new marked.Renderer();
	renderer.code = ({ text, lang }: { text: string; lang?: string }) => {
		const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext';
		const highlighted = hljs.highlight(text, { language }).value;
		const escaped = text.replace(/"/g, '&quot;').replace(/</g, '&lt;');
		return `<div class="code-block group/code relative my-3"><div class="code-head">${language}<button class="copy-btn" data-code="${escaped}">复制</button></div><pre class="code-pre"><code class="hljs language-${language}">${highlighted}</code></pre></div>`;
	};

	marked.setOptions({ renderer, gfm: true, breaks: true });

	let html = $derived.by(() => {
		if (!content) return '';
		try {
			let src = content;
			if (streaming) {
				const fenceCount = (src.match(/```/g) || []).length;
				if (fenceCount % 2 !== 0) src += '\n```';
			}
			return marked.parse(src, { async: false }) as string;
		} catch {
			return content;
		}
	});

	function handleClick(e: MouseEvent) {
		const btn = (e.target as HTMLElement).closest('.copy-btn') as HTMLElement | null;
		if (!btn) return;
		const code = btn.dataset.code ?? '';
		const decoded = code.replace(/&quot;/g, '"').replace(/&lt;/g, '<');
		navigator.clipboard.writeText(decoded);
		btn.textContent = '已复制';
		setTimeout(() => { btn.textContent = '复制'; }, 2000);
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
