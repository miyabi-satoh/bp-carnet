<script lang="ts">
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import type { Snippet } from 'svelte';

	/** 戻せない操作の確認モーダルの見出しの行。赤い丸の警告アイコンを見出しの左に並べる。
	 * 見出しは Dialog と AlertDialog で部品が違うため`children` で渡す。 */
	let { children }: { children: Snippet } = $props();
</script>

<div class="flex items-center gap-3">
	<div
		class="flex size-10 shrink-0 items-center justify-center rounded-full bg-destructive/10 text-destructive"
		aria-hidden="true"
	>
		<TriangleAlertIcon class="size-5" strokeWidth={2.2} />
	</div>
	{@render children()}
</div>
