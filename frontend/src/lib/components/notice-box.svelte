<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';

	/** アイコン付きの注意の枠。`destructive` は消える記録の警告、`muted` は次の操作の案内。 */
	let {
		variant,
		message
	}: {
		variant: 'destructive' | 'muted';
		message: string;
	} = $props();
</script>

<div
	class={[
		'flex items-start gap-2.5 rounded-md px-3.5 py-3',
		variant === 'destructive' ? 'bg-destructive/10 text-destructive' : 'bg-muted'
	]}
	role="note"
>
	<CircleAlertIcon size={18} class="mt-0.5 shrink-0" aria-hidden="true" />
	<p class="text-sm leading-relaxed">{message}</p>
</div>
