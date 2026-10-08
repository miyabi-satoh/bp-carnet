<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as m from '$lib/paraglide/messages.js';
	import type { Snippet } from 'svelte';

	let {
		loading = false,
		oncancel,
		children
	}: {
		/** 送信中はキャンセルも押せなくする。 */
		loading?: boolean;
		oncancel: () => void;
		/** 実行ボタン。 */
		children: Snippet;
	} = $props();
</script>

<!-- `<form>` の中に置くと Dialog.Content の grid gap が効かないため、上余白を自前で持つ。 -->
<Dialog.Footer class="mt-4">
	<Button type="button" variant="outline" disabled={loading} onclick={oncancel}>
		{m.common_cancel_button()}
	</Button>
	{@render children()}
</Dialog.Footer>
