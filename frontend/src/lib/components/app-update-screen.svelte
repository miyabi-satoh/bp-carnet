<script lang="ts">
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import CircleArrowUpIcon from '@lucide/svelte/icons/circle-arrow-up';

	// アプリの版が古く、サーバーに断られたときに、どの画面の代わりにも出す (docs/mobile-app.md)。
</script>

<CenteredScreen>
	<AuthPageHeader
		icon={CircleArrowUpIcon}
		title={m.layout_update_title()}
		subtitle={m.layout_update_description()}
	/>
</CenteredScreen>
