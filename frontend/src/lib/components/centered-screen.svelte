<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';

	/** ログイン・登録などの、中身を画面の真ん中に置く画面の外枠。 */
	let { children, ...restProps }: Omit<HTMLAttributes<HTMLDivElement>, 'class'> = $props();
</script>

<div
	class="flex min-h-svh flex-col items-center justify-center px-6 pt-safe-6 pb-safe-max-6"
	{...restProps}
>
	{@render children?.()}
</div>
