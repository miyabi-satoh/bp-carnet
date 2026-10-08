<script lang="ts">
	import { resolve } from '$app/paths';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** 知らせるだけのダイアログ (通信・保存の失敗、操作の完了)。タイトルと本文以外は
	 * どの画面でも同じ (閉じるボタン1つ) なので共通化した。開く操作は
	 * `record-form-dialog.svelte`・`logout-dialog.svelte` と同じ `bind:this` +
	 * 命令的メソッドのパターンに揃える (呼び出し側に open/title/message の
	 * 3つの `$state` を持たせないため)。 */
	type Props = {
		/** 問い合わせ先の URL (`/auth/providers` の `contactUrl`)。`contact` を付けて開いたときに使う。
		 * 開いた後に届いても効くよう、`show` の引数でなくプロップで受ける。取れなければ `null` で、リンクを出さない。 */
		contactUrl?: string | null;
	};

	let { contactUrl = null }: Props = $props();

	let open = $state(false);
	let title = $state('');
	let message = $state('');
	let helpSlug = $state<string | undefined>(undefined);
	let contact = $state(false);

	/** `helpSlug` を渡すと、本文の下に使い方の項目 (`/help/<slug>`) へのリンクを出す。
	 * `contact` を付けると、本文に問い合わせの一文とフォームへのリンクを足す (docs/authentication.md)。 */
	export function show(
		noticeTitle: string,
		noticeMessage: string,
		options: { helpSlug?: string; contact?: boolean } = {}
	) {
		title = noticeTitle;
		message = noticeMessage;
		helpSlug = options.helpSlug;
		contact = options.contact ?? false;
		open = true;
	}
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{title}</AlertDialog.Title>
			<AlertDialog.Description>
				{message}{#if contact}{m.common_contact_operator()}{/if}
			</AlertDialog.Description>
			{#if contact && contactUrl}
				<!-- 外のサイトなので resolve() は通さない。 -->
				<!-- eslint-disable svelte/no-navigation-without-resolve -->
				<a
					href={contactUrl}
					target="_blank"
					rel="noopener"
					class="text-sm text-accent-foreground underline hover:text-primary"
					>{m.common_contact_form_link()}<span class="sr-only">{m.common_opens_in_new_tab()}</span
					></a
				>
				<!-- eslint-enable svelte/no-navigation-without-resolve -->
			{/if}
			{#if helpSlug}
				<a
					href={resolve('/help/[slug]', { slug: helpSlug })}
					class="text-sm text-accent-foreground underline hover:text-primary"
					>{m.notice_help_link()}</a
				>
			{/if}
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Action onclick={() => (open = false)}
				>{m.common_close_button()}</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
