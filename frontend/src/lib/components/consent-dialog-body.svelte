<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import NewTabLink from '$lib/components/new-tab-link.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** Gemini API へ送る前の同意のダイアログ (写真は `ocr-consent-dialog.svelte`、ファイルは
	 * `file-read-consent-dialog.svelte`) の中身。送信先と Google の扱いは、送るものによらず同じなのでここに持つ。
	 * 開閉と、同意したときの処理は、置く側のダイアログが持つ。 */
	let {
		title,
		lead,
		data,
		purpose,
		notStored,
		note,
		onsubmit,
		footer
	}: {
		title: string;
		lead: string;
		/** 「送信するもの」の行。 */
		data: string;
		/** 「目的」の行。 */
		purpose: string;
		/** 運営者が保存しないことの文。Google の扱いの文を後ろに続ける。 */
		notStored: string;
		/** そのダイアログだけの補足 (同意の取り消し方・使う枠)。 */
		note: string;
		onsubmit: (event: SubmitEvent) => void;
		/** 「同意する」(`type="submit"`) と「同意しない」のボタン。 */
		footer: Snippet;
	} = $props();
</script>

<Dialog.Header>
	<Dialog.Title>{title}</Dialog.Title>
	<Dialog.Description>{lead}</Dialog.Description>
</Dialog.Header>

<ul class="flex flex-col gap-2 text-sm">
	<li>{m.consent_dialog_body_item_recipient()}</li>
	<li>{data}</li>
	<li>{purpose}</li>
</ul>
<ul class="flex flex-col gap-2 text-sm text-muted-foreground">
	<li>
		<!-- Google が改善に使わないのは、運営者が有料の条件で使っているため (privacy.md)。 -->
		{notStored}{m.consent_dialog_body_item_not_trained()}
	</li>
	<li>{note}</li>
</ul>

<form {onsubmit}>
	<p class="text-xs leading-relaxed text-muted-foreground">
		<NewTabLink href={resolve('/privacy')}>{m.privacy_title()}</NewTabLink>
	</p>

	<!-- `<form>` の中に置くと Dialog.Content の grid gap が効かないため、上余白を自前で持つ
	     (DialogFormFooter と同じ)。取り消しのボタンの文言がダイアログごとに違うので、DialogFormFooter は使わない。 -->
	<Dialog.Footer class="mt-4">
		{@render footer()}
	</Dialog.Footer>
</form>
