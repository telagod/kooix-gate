#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Kooix Gate · 空衍 logo generator (v2 — 0.4.101)
 *
 * 设计语言 —— 「空」与「衍」的几何对话：
 *
 * - **空**：中心是同心方形的负空间（rounded square inset），不画实心点，
 *   让观看者眼睛"落到空里"。这是 LLM 网关的"路由"隐喻——所有请求都
 *   先经过这道空门。
 *
 * - **衍**：四个螺旋臂从中心向外推衍，每个臂用近似 archimedean spiral，
 *   跨越 ~145° 角度，自然形成"风车 / 旋涡"——表达 token 流式生成的推
 *   演节奏。臂之间留 ~75° 空隙避免拥挤。
 *
 * - **栅**：对角线 4 主 + 4 副网格节点，模拟"网关入口"——
 *   多 channel 离散流量分发。
 *
 * - **气**：四个主基本方向 (上下左右) 用虚线短戟营造"光晕呼吸"，
 *   仅 8 段 stroke，克制不喧宾夺主。
 *
 * 配色：zinc 单色，currentColor 让父级 CSS 切换 light/dark 自动反转。
 */

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const componentPath = resolve(root, 'src/lib/components/brand/KooixLogo.svelte');
const logoPath = resolve(root, 'src/lib/assets/kooix-logo.svg');
const faviconPath = resolve(root, 'src/lib/assets/favicon.svg');

const VB = 256;
const C = { x: VB / 2, y: VB / 2 };
const fmt = (n) => Number(n.toFixed(2)).toString();
const pstr = (x, y) => `${fmt(x)} ${fmt(y)}`;

// ────────────────────────────────────────────────────────────────────────────
// 1. 中心负空间方框 —— 「空」
// ────────────────────────────────────────────────────────────────────────────
function centerVoid() {
	const outerSize = 38;
	const outerR = 6;
	const innerSize = 22;
	const innerR = 4;
	const half1 = outerSize / 2;
	const half2 = innerSize / 2;
	return `<g class="void">
		<rect x="${fmt(C.x - half1)}" y="${fmt(C.y - half1)}" width="${fmt(outerSize)}" height="${fmt(outerSize)}" rx="${outerR}" ry="${outerR}" fill="none" stroke="currentColor" stroke-width="3"/>
		<rect x="${fmt(C.x - half2)}" y="${fmt(C.y - half2)}" width="${fmt(innerSize)}" height="${fmt(innerSize)}" rx="${innerR}" ry="${innerR}" fill="currentColor" opacity="0.16"/>
	</g>`;
}

// ────────────────────────────────────────────────────────────────────────────
// 2. 螺旋臂 —— 「衍」
//
// 调参（v2 final）：
//   - 起 r=26（贴中心方框外约 4px）
//   - 终 r=86（外圈节点 r=104 之内 18px，留视觉衔接空间）
//   - sweep=95°（4 臂 × 95° = 380°，不会缠绕；相邻臂端起距 60px）
//   - 端点离 viewBox 边 ≥ 42px，安全不被裁
// ────────────────────────────────────────────────────────────────────────────
function spiralArm(startDeg) {
	const a = 26;
	const b = 60;
	const sweepDeg = 95;
	const steps = 32;
	const points = [];
	for (let i = 0; i <= steps; i += 1) {
		const t = i / steps;
		const r = a + b * t;
		const theta = ((startDeg + sweepDeg * t) * Math.PI) / 180;
		points.push({ x: C.x + Math.cos(theta) * r, y: C.y + Math.sin(theta) * r });
	}
	const tension = 0.6;
	let d = `M${pstr(points[0].x, points[0].y)}`;
	for (let i = 0; i < points.length - 1; i += 1) {
		const p0 = points[Math.max(0, i - 1)];
		const p1 = points[i];
		const p2 = points[i + 1];
		const p3 = points[Math.min(points.length - 1, i + 2)];
		const cp1x = p1.x + ((p2.x - p0.x) / 6) * tension;
		const cp1y = p1.y + ((p2.y - p0.y) / 6) * tension;
		const cp2x = p2.x - ((p3.x - p1.x) / 6) * tension;
		const cp2y = p2.y - ((p3.y - p1.y) / 6) * tension;
		d += ` C${pstr(cp1x, cp1y)} ${pstr(cp2x, cp2y)} ${pstr(p2.x, p2.y)}`;
	}
	const tip = points[points.length - 1];
	return { path: d, tip };
}

function arms() {
	const startAngles = [-90, 0, 90, 180];
	const parts = [];
	for (const a of startAngles) {
		const { path, tip } = spiralArm(a - 8);
		parts.push(
			`<path d="${path}" fill="none" stroke="currentColor" stroke-width="3.4" stroke-linecap="round" opacity="0.92"/>`
		);
		parts.push(
			`<circle cx="${fmt(tip.x)}" cy="${fmt(tip.y)}" r="3.6" fill="currentColor"/>`
		);
	}
	return `<g class="arms">\n\t\t${parts.join('\n\t\t')}\n\t</g>`;
}

// ────────────────────────────────────────────────────────────────────────────
// 3. 外圈网格节点 —— 「栅」
// ────────────────────────────────────────────────────────────────────────────
function gateNodes() {
	const nodes = [];
	const diagDegs = [45, 135, 225, 315];
	for (const deg of diagDegs) {
		const theta = (deg * Math.PI) / 180;
		const r1 = 104;
		const cx1 = C.x + Math.cos(theta) * r1;
		const cy1 = C.y + Math.sin(theta) * r1;
		const s = 5.6;
		nodes.push(
			`<rect x="${fmt(cx1 - s / 2)}" y="${fmt(cy1 - s / 2)}" width="${fmt(s)}" height="${fmt(s)}" rx="1.3" ry="1.3" fill="currentColor" transform="rotate(45 ${fmt(cx1)} ${fmt(cy1)})"/>`
		);
		const r2 = 118;
		const cx2 = C.x + Math.cos(theta) * r2;
		const cy2 = C.y + Math.sin(theta) * r2;
		nodes.push(
			`<circle cx="${fmt(cx2)}" cy="${fmt(cy2)}" r="2.4" fill="currentColor" opacity="0.55"/>`
		);
	}
	return `<g class="gates">\n\t\t${nodes.join('\n\t\t')}\n\t</g>`;
}

// ────────────────────────────────────────────────────────────────────────────
// 4. 灵气短戟 —— 「气」
// ────────────────────────────────────────────────────────────────────────────
function aura() {
	const cardinals = [
		[0, -1],
		[1, 0],
		[0, 1],
		[-1, 0]
	];
	const segs = [];
	for (const [dx, dy] of cardinals) {
		const r1 = 110;
		const r2 = 118;
		const r3 = 122;
		const r4 = 126;
		const x1 = C.x + dx * r1;
		const y1 = C.y + dy * r1;
		const x2 = C.x + dx * r2;
		const y2 = C.y + dy * r2;
		const x3 = C.x + dx * r3;
		const y3 = C.y + dy * r3;
		const x4 = C.x + dx * r4;
		const y4 = C.y + dy * r4;
		segs.push(
			`<line x1="${fmt(x1)}" y1="${fmt(y1)}" x2="${fmt(x2)}" y2="${fmt(y2)}" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" opacity="0.85"/>`
		);
		segs.push(
			`<line x1="${fmt(x3)}" y1="${fmt(y3)}" x2="${fmt(x4)}" y2="${fmt(y4)}" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" opacity="0.5"/>`
		);
	}
	return `<g class="aura">\n\t\t${segs.join('\n\t\t')}\n\t</g>`;
}

// ────────────────────────────────────────────────────────────────────────────
// 5. 完整 logo
// ────────────────────────────────────────────────────────────────────────────
const LOGO_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VB} ${VB}" role="img" aria-label="Kooix Gate · 空衍 logo">
  <title>Kooix Gate · 空衍</title>
  <style>
    .mark { color: #09090b; }
    @media (prefers-color-scheme: dark) { .mark { color: #fafafa; } }
  </style>
  <!-- Generated by web/scripts/generate-kooix-logo.mjs. Edit the generator, not these paths. -->
  <g class="mark">
	${arms()}
	${aura()}
	${gateNodes()}
	${centerVoid()}
  </g>
</svg>
`;

// ────────────────────────────────────────────────────────────────────────────
// 6. Favicon — 简化版
// ────────────────────────────────────────────────────────────────────────────
function faviconArm(startDeg) {
	// 64×64 viewBox 缩放：起 r=7、终 r=22、sweep=85°
	const a = 7;
	const b = 15;
	const sweepDeg = 85;
	const steps = 16;
	const points = [];
	for (let i = 0; i <= steps; i += 1) {
		const t = i / steps;
		const r = a + b * t;
		const theta = ((startDeg + sweepDeg * t) * Math.PI) / 180;
		points.push({ x: 32 + Math.cos(theta) * r, y: 32 + Math.sin(theta) * r });
	}
	let d = `M${pstr(points[0].x, points[0].y)}`;
	for (let i = 1; i < points.length; i += 1) {
		d += ` L${pstr(points[i].x, points[i].y)}`;
	}
	return { path: d, tip: points[points.length - 1] };
}

const FAVICON_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Kooix Gate">
  <title>Kooix Gate · 空衍</title>
  <style>
    .mark { color: #09090b; }
    @media (prefers-color-scheme: dark) { .mark { color: #fafafa; } }
  </style>
  <g class="mark">
    <g class="arms">
${[-90, 0, 90, 180]
	.map((a) => {
		const { path, tip } = faviconArm(a - 8);
		return `      <path d="${path}" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" opacity="0.94"/>\n      <circle cx="${fmt(tip.x)}" cy="${fmt(tip.y)}" r="1.8" fill="currentColor"/>`;
	})
	.join('\n')}
    </g>
    <g class="void">
      <rect x="24" y="24" width="16" height="16" rx="2.6" ry="2.6" fill="none" stroke="currentColor" stroke-width="2.2"/>
      <rect x="28" y="28" width="8" height="8" rx="1.6" ry="1.6" fill="currentColor" opacity="0.2"/>
    </g>
  </g>
</svg>
`;

// ────────────────────────────────────────────────────────────────────────────
// 7. Svelte 组件包装 — 直接 import 用
//
// 兼容旧调用方：保留 `tone` prop（'mark' | 'tile'）作为视觉变体。
// - 'mark'（默认）：透明背景，纯线条 + 节点，currentColor 跟父级
// - 'tile'：圆角方块底（zinc-100 / zinc-800），头像 / sidebar 槽位适用
// ────────────────────────────────────────────────────────────────────────────
const COMPONENT = `<!-- Generated by web/scripts/generate-kooix-logo.mjs -->
<script lang="ts">
	let {
		size = 32,
		class: className = '',
		tone = 'mark',
		title = 'Kooix Gate · 空衍'
	}: {
		size?: number;
		class?: string;
		tone?: 'mark' | 'tile';
		title?: string;
	} = $props();

	const tileBg =
		'inline-flex items-center justify-center rounded-xl bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100';
	const tilePadding = 4;
</script>

{#if tone === 'tile'}
	<span
		class="{tileBg} {className}"
		style="width: {size + tilePadding * 2}px; height: {size + tilePadding * 2}px;"
		aria-label={title}
	>
		<svg
			xmlns="http://www.w3.org/2000/svg"
			viewBox="0 0 ${VB} ${VB}"
			width={size}
			height={size}
			role="img"
			aria-hidden="true"
		>
			<title>{title}</title>
			${arms()}
			${aura()}
			${gateNodes()}
			${centerVoid()}
		</svg>
	</span>
{:else}
	<svg
		xmlns="http://www.w3.org/2000/svg"
		viewBox="0 0 ${VB} ${VB}"
		width={size}
		height={size}
		role="img"
		aria-label={title}
		class={className}
	>
		<title>{title}</title>
		${arms()}
		${aura()}
		${gateNodes()}
		${centerVoid()}
	</svg>
{/if}
`;

writeFileSync(logoPath, LOGO_SVG);
writeFileSync(faviconPath, FAVICON_SVG);
writeFileSync(componentPath, COMPONENT);

console.log(`generated:
  - ${logoPath}
  - ${faviconPath}
  - ${componentPath}`);
