<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import { logout } from '$lib/auth';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { busyCloseGuard } from '$lib/dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** ログアウトの確認と実行、失敗時のエラーダイアログをまとめたコンポーネント。ヘッダーの
	 * ユーザーメニューと設定画面の両方から、トリガーボタンだけを変えて使う
	 * (`record-form-dialog.svelte` と同じ `bind:this` + 命令的メソッドのパターン)。
	 * ログアウトは短い確認のダイアログを挟む。 */
	let open = $state(false);
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	export function show() {
		open = true;
	}

	/** 送るまでダイアログを開いたままにし、失敗したらその上にエラーを重ねて、その場でやり直せる
	 * ようにする。 */
	async function confirm() {
		if (loading) return;
		loading = true;
		const ok = await logout();
		loading = false;
		if (!ok) {
			errorDialog?.show(m.common_logout_error_title(), GENERIC_ERROR_MESSAGE());
			return;
		}
		// 自分で閉じる (ヘッダーの分はルートのレイアウトにあり、ログイン画面へ移っても残るため)。
		open = false;
		await goto(resolve('/login'));
	}
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content {...closeGuard}>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.logout_dialog_title()}</AlertDialog.Title>
			<AlertDialog.Description>{m.logout_dialog_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={loading}>{m.common_cancel_button()}</AlertDialog.Cancel>
			<LoadingButton {loading} onclick={confirm}>{m.logout_dialog_confirm_button()}</LoadingButton>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<NoticeDialog bind:this={errorDialog} />
