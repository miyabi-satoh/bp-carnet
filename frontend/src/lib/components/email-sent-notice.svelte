<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import PageResult from '$lib/components/page-result.svelte';
	import { isNativeApp } from '$lib/native-app';
	import * as m from '$lib/paraglide/messages.js';

	type Props = {
		title: string;
		description: string;
		sentTo: string;
		/** アプリで開いているときに添える、ブラウザからアプリへ戻る案内。 */
		appNote: string;
		/** 「メールアドレスを直す」を押したとき。押したボタンが消えるので、呼び出し側でメール欄に移す。 */
		onedit: () => void;
	};

	let { title, description, sentTo, appNote, onedit }: Props = $props();
</script>

<!-- メールを送った後の表示。打ち間違いに気づけるよう送り先を見せ、直して送り直せるようにする。 -->
<PageResult {title} {description} />
<p class="mt-3 text-sm break-all">
	{m.email_request_sent_to_label()}<span class="font-bold">{sentTo}</span>
</p>
<!-- メールのリンクはブラウザで開く。アプリへ戻る案内を添える (docs/mobile-app.md)。 -->
{#if isNativeApp}
	<p class="mt-2 text-sm text-muted-foreground">{appNote}</p>
{/if}
<Button type="button" variant="outline" class="mt-5 w-full text-base" onclick={onedit}
	>{m.email_request_edit_button()}</Button
>
