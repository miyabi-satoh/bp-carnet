<script lang="ts">
	import ConsentDialogBody from '$lib/components/consent-dialog-body.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { busyCloseGuard } from '$lib/dialog';
	import { consentToOcr } from '$lib/ocr';
	import * as m from '$lib/paraglide/messages.js';

	/** 写真を初めて読み取る前に、送り先 (Google の Gemini API) を示して同意を得るダイアログ
	 * (docs/ocr.md)。`bind:this` + `show()` で開く。
	 * 同意を記録できたら `onagreed`、「使わない」なら `ondeclined` を呼ぶ。 */
	let {
		onagreed,
		ondeclined
	}: {
		onagreed: () => void;
		ondeclined: () => void;
	} = $props();

	let open = $state(false);
	let saving = $state(false);
	const closeGuard = busyCloseGuard(() => saving);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	export function show() {
		open = true;
	}

	async function agree(event: SubmitEvent) {
		event.preventDefault();
		if (saving) return;
		saving = true;
		const result = await consentToOcr();
		saving = false;
		open = false;
		if (!result.ok) {
			errorDialog?.show(m.ocr_consent_error_title(), result.message);
			return;
		}
		onagreed();
	}

	function decline() {
		open = false;
		ondeclined();
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content {...closeGuard}>
		<ConsentDialogBody
			title={m.ocr_consent_title()}
			lead={m.ocr_consent_lead()}
			data={m.ocr_consent_item_data()}
			purpose={m.ocr_consent_item_purpose()}
			notStored={m.ocr_consent_item_not_stored()}
			note={m.ocr_consent_item_withdraw()}
			onsubmit={agree}
		>
			{#snippet footer()}
				<Button type="button" variant="outline" disabled={saving} onclick={decline}>
					{m.ocr_consent_decline_button()}
				</Button>
				<LoadingButton type="submit" loading={saving}>
					{m.ocr_consent_agree_button()}
				</LoadingButton>
			{/snippet}
		</ConsentDialogBody>
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
