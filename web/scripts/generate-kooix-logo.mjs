#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Kooix logo generator.
 *
 * Design math:
 * - D4 symmetry as the base grammar: every non-central accent is generated once,
 *   then repeated by 90deg rotation.
 * - Core: polar super-star r(θ)=inner+(outer-inner)|cos(2θ)|^γ.
 *   This gives a clean four-point star with concave negative space.
 * - Orbit: tapered polar ribbon around diagonal axes. The width is
 *   w(t)=min+(max-min)sin(πt)^0.62, so the ends are pointed and the middle is calm.
 * - Nodes/dust: polar points at cardinal/diagonal extrema, never hand-placed.
 */

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const componentPath = resolve(root, 'src/lib/components/brand/KooixLogo.svelte');
const logoPath = resolve(root, 'src/lib/assets/kooix-logo.svg');
const faviconPath = resolve(root, 'src/lib/assets/favicon.svg');

const C = { x: 128, y: 128 };
const fmt = (n) => Number(n.toFixed(2)).toString();
const point = (x, y) => ({ x, y });
const polar = (deg, radius) => {
	const a = (deg * Math.PI) / 180;
	return point(C.x + Math.cos(a) * radius, C.y + Math.sin(a) * radius);
};
const pstr = ({ x, y }) => `${fmt(x)} ${fmt(y)}`;
const attrPoint = ({ x, y }) => `cx="${fmt(x)}" cy="${fmt(y)}"`;

const closedCatmullRom = (pts, tension = 0.72) => {
	const n = pts.length;
	const d = [`M${pstr(pts[0])}`];
	for (let i = 0; i < n; i += 1) {
		const p0 = pts[(i - 1 + n) % n];
		const p1 = pts[i];
		const p2 = pts[(i + 1) % n];
		const p3 = pts[(i + 2) % n];
		const c1 = point(p1.x + ((p2.x - p0.x) * tension) / 6, p1.y + ((p2.y - p0.y) * tension) / 6);
		const c2 = point(p2.x - ((p3.x - p1.x) * tension) / 6, p2.y - ((p3.y - p1.y) * tension) / 6);
		d.push(`C${pstr(c1)} ${pstr(c2)} ${pstr(p2)}`);
	}
	d.push('Z');
	return d.join(' ');
};

const openCatmullRom = (pts, tension = 0.62) => {
	const d = [`M${pstr(pts[0])}`];
	for (let i = 0; i < pts.length - 1; i += 1) {
		const p0 = pts[Math.max(0, i - 1)];
		const p1 = pts[i];
		const p2 = pts[i + 1];
		const p3 = pts[Math.min(pts.length - 1, i + 2)];
		const c1 = point(p1.x + ((p2.x - p0.x) * tension) / 6, p1.y + ((p2.y - p0.y) * tension) / 6);
		const c2 = point(p2.x - ((p3.x - p1.x) * tension) / 6, p2.y - ((p3.y - p1.y) * tension) / 6);
		d.push(`C${pstr(c1)} ${pstr(c2)} ${pstr(p2)}`);
	}
	return d.join(' ');
};

const polarSuperStarPoints = ({ inner, outer, gamma, samples, rotateDeg = -90 }) => {
	const pts = [];
	for (let i = 0; i < samples; i += 1) {
		const theta = (i / samples) * Math.PI * 2 + (rotateDeg * Math.PI) / 180;
		const wave = Math.abs(Math.cos(2 * theta));
		const r = inner + (outer - inner) * wave ** gamma;
		pts.push(point(C.x + Math.cos(theta) * r, C.y + Math.sin(theta) * r));
	}
	return pts;
};

const taperedRibbonPath = ({ centerDeg, spanDeg, radius, minWidth, maxWidth, samples, bow = 4 }) => {
	const outer = [];
	const inner = [];
	for (let i = 0; i <= samples; i += 1) {
		const t = i / samples;
		const u = Math.sin(Math.PI * t);
		const deg = centerDeg - spanDeg / 2 + spanDeg * t;
		const mid = radius + bow * Math.sin(Math.PI * (t - 0.5));
		const width = minWidth + (maxWidth - minWidth) * u ** 0.62;
		outer.push(polar(deg, mid + width / 2));
		inner.unshift(polar(deg, mid - width / 2));
	}
	return closedCatmullRom([...outer, ...inner], 0.55);
};

const diamond = ({ deg, radius, size, opacity = 1 }) => {
	const p = polar(deg, radius);
	const half = size / 2;
	return `<rect x="${fmt(p.x - half)}" y="${fmt(p.y - half)}" width="${fmt(size)}" height="${fmt(size)}" rx="${fmt(size * 0.12)}" opacity="${fmt(opacity)}" transform="rotate(45 ${fmt(p.x)} ${fmt(p.y)})"/>`;
};

const outerStar = closedCatmullRom(polarSuperStarPoints({ inner: 23, outer: 78, gamma: 1.72, samples: 88 }), 0.72);
const innerVoid = closedCatmullRom(polarSuperStarPoints({ inner: 5.5, outer: 17, gamma: 1.55, samples: 48 }).reverse(), 0.7);
const corePath = `${outerStar} ${innerVoid}`;
const blade = taperedRibbonPath({ centerDeg: 45, spanDeg: 70, radius: 103, minWidth: 1.4, maxWidth: 13.5, samples: 16, bow: 3.2 });
const halo = openCatmullRom(Array.from({ length: 34 }, (_, i) => polar(-29 + (58 * i) / 33, 103)), 0.4);

const rotations = [0, 90, 180, 270];
const rotatedPaths = (d, attrs = '') => rotations.map((r) => `<path d="${d}"${r ? ` transform="rotate(${r} 128 128)"` : ''}${attrs}/>`).join('\n\t\t');
const nodes = [0, 90, 180, 270]
	.map((deg) => `<circle ${attrPoint(polar(deg, 104))} r="5.8"/>`)
	.join('\n\t\t');
const dust = [45, 135, 225, 315]
	.map((deg) => `${diamond({ deg, radius: 72, size: 5.8, opacity: 0.58 })}\n\t\t${diamond({ deg, radius: 91, size: 3.8, opacity: 0.42 })}`)
	.join('\n\t\t');

const markInner = (color = 'currentColor') => `<g fill="${color}" fill-rule="evenodd">
		<path d="${corePath}"/>
	</g>
	<g fill="${color}" opacity="0.94">
		${rotatedPaths(blade)}
	</g>
	<g fill="none" stroke="${color}" stroke-width="2.2" stroke-linecap="round" opacity="0.24">
		${rotatedPaths(halo)}
	</g>
	<g fill="${color}">
		${nodes}
	</g>
	<g fill="${color}">
		${dust}
	</g>`;

const svelte = `<script lang="ts">
	import { cn } from '$lib/design';

	type LogoTone = 'plain' | 'tile';

	let {
		class: className = '',
		tone = 'plain',
		title = 'Kooix 空衍',
		size = 24
	}: {
		class?: string;
		tone?: LogoTone;
		title?: string;
		size?: number | string;
	} = $props();

	const rootCls = $derived(cn('inline-block shrink-0', className));
	const tileCls = $derived(
		cn(
			'fill-zinc-50 stroke-zinc-200 dark:fill-zinc-950 dark:stroke-zinc-800',
			tone === 'tile' ? 'opacity-100' : 'opacity-0'
		)
	);
</script>

<!-- Generated by web/scripts/generate-kooix-logo.mjs. Edit the generator, not these points. -->
<svg
	class={rootCls}
	width={size}
	height={size}
	viewBox="0 0 256 256"
	role="img"
	aria-label={title}
	xmlns="http://www.w3.org/2000/svg"
>
	<title>{title}</title>
	<rect x="8" y="8" width="240" height="240" rx="60" class={tileCls} stroke-width="3" />
	${markInner()}
</svg>
`;

const logo = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" role="img" aria-label="Kooix 空衍 logo">
  <title>Kooix 空衍</title>
  <style>
    .mark { color: #09090b; }
    @media (prefers-color-scheme: dark) { .mark { color: #fafafa; } }
  </style>
  <!-- Generated by web/scripts/generate-kooix-logo.mjs. Edit the generator, not these points. -->
  <g class="mark">
	${markInner()}
  </g>
</svg>
`;

const favicon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <rect width="256" height="256" rx="60" fill="#09090b"/>
  <!-- Generated by web/scripts/generate-kooix-logo.mjs. Edit the generator, not these points. -->
  ${markInner('#fafafa')}
</svg>
`;

writeFileSync(componentPath, svelte);
writeFileSync(logoPath, logo);
writeFileSync(faviconPath, favicon);
console.log('generated', componentPath);
console.log('generated', logoPath);
console.log('generated', faviconPath);
