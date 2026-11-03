<script lang="ts">
	import { halfWidthInput } from '#lib/half-width.js';

	/** テスト用: 親に bind した値を表示する。 */
	let value = $state('');
</script>

<input aria-label="値" bind:value {@attach halfWidthInput} />
<p data-testid="value">{value}</p>
