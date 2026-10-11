<script lang="ts">
	import { resolve } from '$app/paths';
	import NewTabLink from '$lib/components/new-tab-link.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { SAMPLE_LINES } from '$lib/import-table';
	import * as m from '$lib/paraglide/messages.js';

	/** 他のアプリのファイルを読み取る前に、何をどこへ送るかを示して同意を得るダイアログ
	 * (docs/import-export.md「他のアプリのファイル」)。`bind:this` + `show(kind)` で開く。
	 *
	 * 写真の同意 (`ocr-consent-dialog.svelte`) と違い、同意を記録しない。何度もする操作でなく、
	 * 送るものの重さ (先頭の数行か、ファイルの全体か) もそのつど違うので、送るたびに確かめる。 */
	let {
		onagreed,
		ondeclined
	}: {
		onagreed: () => void;
		ondeclined: () => void;
	} = $props();

	/** `table` は表の形のテキストの先頭の数行を、`document` は PDF の全体を送る。 */
	type Kind = 'table' | 'document';

	let open = $state(false);
	let kind = $state<Kind>('table');
	/** ボタンで閉じたか。枠の外やエスケープで閉じたときは「送らない」として扱う。 */
	let answered = false;

	export function show(nextKind: Kind) {
		kind = nextKind;
		answered = false;
		open = true;
	}

	function agree(event: SubmitEvent) {
		event.preventDefault();
		answered = true;
		open = false;
		onagreed();
	}

	function decline() {
		answered = true;
		open = false;
		ondeclined();
	}
</script>

<Dialog.Root
	bind:open
	onOpenChange={(isOpen) => {
		if (!isOpen && !answered) decline();
	}}
>
	<!-- 枠の外を押しただけでは閉じない。うっかり閉じると、選んだファイルを選び直すことになるため。 -->
	<Dialog.Content onInteractOutside={(e) => e.preventDefault()}>
		<Dialog.Header>
			<Dialog.Title>{m.file_read_consent_title()}</Dialog.Title>
			<Dialog.Description>
				{kind === 'table' ? m.file_read_consent_table_lead() : m.file_read_consent_document_lead()}
			</Dialog.Description>
		</Dialog.Header>

		<ul class="flex flex-col gap-2 text-sm">
			<li>{m.ocr_consent_item_recipient()}</li>
			<li>
				{kind === 'table'
					? m.file_read_consent_table_item_data({ lines: SAMPLE_LINES })
					: m.file_read_consent_document_item_data()}
			</li>
			<li>
				{kind === 'table'
					? m.file_read_consent_table_item_purpose()
					: m.file_read_consent_document_item_purpose()}
			</li>
		</ul>
		<ul class="flex flex-col gap-2 text-sm text-muted-foreground">
			<li>
				<!-- Google が改善に使わないのは、運営者が有料の条件で使っているため (privacy.md)。 -->
				{m.file_read_consent_item_not_stored()}{m.ocr_consent_item_not_trained()}
			</li>
			<li>{m.file_read_consent_item_quota()}</li>
		</ul>

		<form onsubmit={agree}>
			<p class="text-xs leading-relaxed text-muted-foreground">
				<NewTabLink href={resolve('/privacy')}>{m.privacy_title()}</NewTabLink>
			</p>

			<!-- `<form>` の中に置くと Dialog.Content の grid gap が効かないため、上余白を自前で持つ
			     (`ocr-consent-dialog.svelte` と同じ)。 -->
			<Dialog.Footer class="mt-4">
				<Button type="button" variant="outline" onclick={decline}>
					{m.file_read_consent_decline_button()}
				</Button>
				<Button type="submit">{m.file_read_consent_agree_button()}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
