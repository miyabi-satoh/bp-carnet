<script lang="ts">
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';
	import { setMode, userPrefersMode } from 'mode-watcher';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { buttonVariants } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={buttonVariants({ variant: 'outline', size: 'icon' })}>
		<SunIcon class="size-[1.2rem] scale-100 rotate-0 transition-all dark:scale-0 dark:-rotate-90" />
		<MoonIcon
			class="absolute size-[1.2rem] scale-0 rotate-90 transition-all dark:scale-100 dark:rotate-0"
		/>
		<span class="sr-only">{m.theme_toggle_label()}</span>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.RadioGroup bind:value={() => userPrefersMode.current, setMode}>
			<DropdownMenu.RadioItem value="light">{m.theme_light()}</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="dark">{m.theme_dark()}</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="system">{m.theme_system()}</DropdownMenu.RadioItem>
		</DropdownMenu.RadioGroup>
	</DropdownMenu.Content>
</DropdownMenu.Root>
