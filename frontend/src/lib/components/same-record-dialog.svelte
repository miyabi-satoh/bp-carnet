<script lang="ts">
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** 同じ日時・同じ値の記録がすでにあるとき、登録を続けるかを聞く。
	 * `show()` は「登録する」なら `true`、やめる・閉じるなら `false` で解決する。 */
	let open = $state(false);
	let resolveAnswer: ((register: boolean) => void) | null = null;

	export function show(): Promise<boolean> {
		open = true;
		return new Promise((resolve) => {
			resolveAnswer = resolve;
		});
	}

	function answer(register: boolean) {
		open = false;
		resolveAnswer?.(register);
		resolveAnswer = null;
	}
</script>

<AlertDialog.Root
	bind:open={
		() => open,
		(next) => {
			if (!next) answer(false);
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.same_record_dialog_title()}</AlertDialog.Title>
			<AlertDialog.Description>{m.same_record_dialog_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.common_cancel_button()}</AlertDialog.Cancel>
			<AlertDialog.Action onclick={() => answer(true)}>
				{m.same_record_dialog_confirm_button()}
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
