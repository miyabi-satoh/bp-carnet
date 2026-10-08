<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import type { LoadFailure } from '$lib/auth';
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import CloudOffIcon from '@lucide/svelte/icons/cloud-off';
	import WifiOffIcon from '@lucide/svelte/icons/wifi-off';

	/** 画面を出すのに要る問い合わせ (ログインの確認・使えるログイン方式) が失敗したときに、ページの本文の
	 * 代わりに出す画面 (docs/mobile-app.md)。`onretry` を渡さなければ、layout の load をやり直す。 */
	let {
		failure,
		onretry = invalidateAll
	}: { failure: LoadFailure; onretry?: () => Promise<void> } = $props();

	let retrying = $state(false);

	/** 問い合わせをやり直す。つながれば、この画面が本文に替わる。 */
	async function retry() {
		retrying = true;
		try {
			await onretry();
		} finally {
			retrying = false;
		}
	}
</script>

<CenteredScreen>
	{#if failure === 'offline'}
		<AuthPageHeader
			icon={WifiOffIcon}
			title={m.layout_offline_title()}
			subtitle={m.layout_offline_description()}
		/>
	{:else}
		<AuthPageHeader
			icon={CloudOffIcon}
			title={m.layout_server_error_title()}
			subtitle={m.layout_server_error_description()}
		/>
	{/if}
	<LoadingButton class="w-full max-w-80" loading={retrying} onclick={retry}>
		{m.common_retry_button()}
	</LoadingButton>
</CenteredScreen>
