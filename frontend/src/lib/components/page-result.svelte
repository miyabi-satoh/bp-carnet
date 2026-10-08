<script lang="ts">
	let { title, description }: { title: string; description: string } = $props();
</script>

<!-- 操作が済んだあとに、画面のカードの中身を差し替えて出す結果 (見出しと説明)。カードの枠は呼び出し側が持つ。
     ダイアログの中では `dialog-result.svelte` を使う。 -->
<h2 class="font-bold">{title}</h2>
<p class="mt-2 text-sm text-muted-foreground">{description}</p>
