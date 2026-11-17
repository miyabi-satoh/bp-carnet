<script lang="ts">
	import type { Snippet } from 'svelte';
	import AdminCodeGate from '#lib/components/admin-code-gate.svelte';

	let { children }: { children: Snippet } = $props();
</script>

<!-- 管理画面はどれも、確認コードを通してから出す (docs/authentication.md「管理者の確認コード」)。 -->
<AdminCodeGate>{@render children()}</AdminCodeGate>
