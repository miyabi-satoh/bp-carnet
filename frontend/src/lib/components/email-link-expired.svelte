<script lang="ts">
	import AuthCard from '$lib/components/auth-card.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';

	let {
		description,
		retryHref,
		retryLabel
	}: {
		description: string;
		/** 申し込み直す画面 (サインアップ・再設定の申し込み)。 */
		retryHref: string;
		retryLabel: string;
	} = $props();
</script>

<!-- メールのリンクが期限切れ・使用済みのとき、フォームの代わりに出すカード。申し込み直す先へ導く
     (docs/authentication.md)。 -->
<AuthCard>
	<PageResult title={m.email_link_expired_title()} {description} />
	<!-- 再設定のリンクの画面から再設定の申し込みへは同じルートの移動になり、画面がリンクのトークンを持ったままに
	     なるので、読み込み直す。 -->
	<Button href={retryHref} data-sveltekit-reload class="mt-5 w-full text-base">{retryLabel}</Button>
</AuthCard>
