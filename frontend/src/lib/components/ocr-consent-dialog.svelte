<script lang="ts">
	import { resolve } from '$app/paths';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NewTabLink from '$lib/components/new-tab-link.svelte';
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
		<Dialog.Header>
			<Dialog.Title>{m.ocr_consent_title()}</Dialog.Title>
			<Dialog.Description>{m.ocr_consent_lead()}</Dialog.Description>
		</Dialog.Header>

		<ul class="flex flex-col gap-2 text-sm">
			<li>{m.ocr_consent_item_recipient()}</li>
			<li>{m.ocr_consent_item_data()}</li>
			<li>{m.ocr_consent_item_purpose()}</li>
		</ul>
		<ul class="flex flex-col gap-2 text-sm text-muted-foreground">
			<li>
				<!-- Google が改善に使わないのは、運営者が有料の条件で使っているため (privacy.md)。 -->
				{m.ocr_consent_item_not_stored()}{m.ocr_consent_item_not_trained()}
			</li>
			<li>{m.ocr_consent_item_withdraw()}</li>
		</ul>

		<form onsubmit={agree}>
			<p class="text-xs leading-relaxed text-muted-foreground">
				<NewTabLink href={resolve('/privacy')}>{m.privacy_title()}</NewTabLink>
			</p>

			<!-- `<form>` の中に置くと Dialog.Content の grid gap が効かないため、上余白を自前で持つ
			     (DialogFormFooter と同じ)。取り消しのボタンは「使わない」にするので、DialogFormFooter は使わない。 -->
			<Dialog.Footer class="mt-4">
				<Button type="button" variant="outline" disabled={saving} onclick={decline}>
					{m.ocr_consent_decline_button()}
				</Button>
				<LoadingButton type="submit" loading={saving}>
					{m.ocr_consent_agree_button()}
				</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
