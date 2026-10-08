<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { linkedProviderLabel, unlinkIdentity, type LinkedProvider } from '$lib/auth';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import DialogFormHeader from '$lib/components/dialog-form-header.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import { busyCloseGuard } from '$lib/dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** Google・LINE・Apple との連携を解除する確認のモーダル。設定画面の行から `bind:this` + `show()` で開く
	 * (行ごとに置くと状態が増えるため、画面に1つだけ置いて対象を差し替える)。
	 * 解除は元に戻せる操作 (同じメールアドレスでログインすれば再び連携される) なので、
	 * 削除の確認と違って赤い実行ボタンにも警告アイコンにもしない。 */
	let {
		/** 解除できて閉じきったとき。押したボタンは行ごと消えるので、フォーカスの行き先は呼び出し側が決める。 */
		onunlinked
	}: {
		onunlinked: () => void;
	} = $props();

	let open = $state(false);
	/** 解除できて閉じたか。開いたボタンは消えているので、閉じたときにそこへフォーカスを戻さない。 */
	let unlinked = false;
	let provider = $state<LinkedProvider>('google');
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	export function show(target: LinkedProvider) {
		provider = target;
		unlinked = false;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading) return;

		loading = true;
		const result = await unlinkIdentity(provider);
		if (result.ok) {
			// 一覧は `/auth/me` の値で描くので、閉じる前に読み直す (行が残ったまま閉じない)。
			await invalidateAll();
		}
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.unlink_identity_dialog_error_title(), result.message);
			return;
		}
		unlinked = true;
		open = false;
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		{...closeGuard}
		onCloseAutoFocus={(e) => {
			if (!unlinked) return;
			e.preventDefault();
			onunlinked();
		}}
	>
		<DialogFormHeader
			title={m.unlink_identity_dialog_title({ provider: linkedProviderLabel(provider) })}
			description={m.unlink_identity_dialog_description({
				provider: linkedProviderLabel(provider)
			})}
			closeDisabled={loading}
		/>
		<form onsubmit={submit}>
			{#if provider === 'line'}
				<p class="text-sm text-muted-foreground">{m.unlink_identity_dialog_line_note()}</p>
			{:else if provider === 'apple'}
				<p class="text-sm text-muted-foreground">{m.unlink_identity_dialog_apple_note()}</p>
			{/if}
			<DialogFormFooter {loading} oncancel={() => (open = false)}>
				<LoadingButton type="submit" {loading}>
					{m.unlink_identity_dialog_submit_button()}
				</LoadingButton>
			</DialogFormFooter>
		</form>
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
