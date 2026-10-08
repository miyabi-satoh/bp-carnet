<script lang="ts">
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';
	import type { ComponentProps } from 'svelte';

	type Props = Omit<ComponentProps<typeof Input>, 'type' | 'files'> & {
		class?: string;
	};

	let {
		ref = $bindable(null),
		value = $bindable(''),
		class: className,
		...restProps
	}: Props = $props();

	let visible = $state(false);
</script>

<!-- ADR: 表示切り替えボタンは入力欄の右端に重ねる。InputGroup のように入力欄とボタンを
     横に並べると、Chrome の自動入力の背景色が入力部分にだけ付き、ボタンの手前で途切れる。 -->
<div class={cn('relative', className)}>
	<Input bind:ref bind:value type={visible ? 'text' : 'password'} class="pr-11" {...restProps} />
	<!-- 入力欄の枠線に重ならないよう、ボタンは枠の内側に収める。ホバーで地を塗ると、入力欄の右端だけ色が
	     変わって見えるため塗らない。 -->
	<Button
		type="button"
		variant="ghost"
		size="icon"
		class="absolute top-0.5 right-0.5 size-10 rounded-sm text-muted-foreground hover:bg-transparent dark:hover:bg-transparent"
		aria-label={visible ? m.password_input_hide_label() : m.password_input_show_label()}
		aria-pressed={visible}
		onclick={() => (visible = !visible)}
	>
		{#if visible}
			<EyeOffIcon />
		{:else}
			<EyeIcon />
		{/if}
	</Button>
</div>
