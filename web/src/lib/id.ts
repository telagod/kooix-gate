/**
 * Typed ID utilities.
 *
 * API responses return IDs in `{prefix}_{hex}` format (e.g. `org_019e2c1ba7d17162842207e4b24f5f98`).
 * URL paths and headers still expect raw UUIDs.
 */

/**
 * Extract the raw UUID from a typed ID string.
 * Accepts both `org_019e2c1b...` (strips prefix) and raw UUID (pass-through).
 * The returned string is in standard UUID format with hyphens.
 */
export function rawId(typedId: string): string {
	const idx = typedId.indexOf('_');
	if (idx === -1) return typedId; // already raw UUID
	const hex = typedId.slice(idx + 1);
	if (hex.length === 32) {
		// Convert simple hex to hyphenated UUID: 8-4-4-4-12
		return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
	}
	return hex; // already hyphenated or other format
}

/**
 * Short display version of a typed ID.
 * `org_019e2c1ba7d17162842207e4b24f5f98` → `org_019e2c1b`
 * `019e2c1b-a7d1-7162-8422-07e4b24f5f98` → `019e2c1b`
 */
export function shortId(typedId: string | null | undefined): string {
	if (!typedId) return '—';
	const idx = typedId.indexOf('_');
	if (idx === -1) {
		// Raw UUID: first 8 chars
		return typedId.slice(0, 8);
	}
	// Typed ID: prefix + first 8 hex chars
	return typedId.slice(0, idx + 9);
}
