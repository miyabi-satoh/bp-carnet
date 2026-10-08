/** 必須の印 (`*`、`required-mark.svelte`) が付いた欄のラベル。印は読み上げから外しているが、
 * Playwright の `getByLabel`・vitest browser の `getByLabelText` は label の文字をそのまま見るため、
 * テストで欄を探すときは印まで含める。e2e と単体テストで共用する。 */
export function requiredLabel(label: string): string {
	return `${label}*`;
}
