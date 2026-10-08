<script lang="ts">
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

<!-- 未ログインで開く画面 (ログイン・サインアップ・パスワードの設定と再設定) の本文を載せるカード。 -->
<div class="w-full max-w-80 rounded-lg raised bg-card p-6">
	{@render children()}
</div>
