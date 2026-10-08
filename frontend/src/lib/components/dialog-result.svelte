<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';

	let { title, description }: { title: string; description: string } = $props();
</script>

<!-- 操作が済んだあとに、ダイアログの中身を差し替えて出す結果 (見出し・説明と閉じるボタン)。 -->
<Dialog.Header>
	<Dialog.Title>{title}</Dialog.Title>
	<Dialog.Description>{description}</Dialog.Description>
</Dialog.Header>
<Dialog.Footer showCloseButton />
