<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import HelpExternalLink from '$lib/components/help-external-link.svelte';
	import { APP_STORE_URL } from '$lib/app-store-link';
	import * as m from '$lib/paraglide/messages.js';
</script>

<!-- App Store とホーム画面の操作はアプリの外なので画像を撮れない。 -->

<HelpSteps>

<HelpStep n={1} image={false}>

App Store から BP Carnet のアプリを入れます。

<HelpExternalLink href={APP_STORE_URL}>{m.common_app_store_link()}</HelpExternalLink>

</HelpStep>

<HelpStep n={2} image={false}>

アプリを開き、ブラウザーで使っていたのと同じ方法でログインします。記録も、購入した読み取りの枠も、そのまま使えます。

</HelpStep>

<HelpStep n={3} image={false}>

Safari から「ホーム画面に追加」したアイコンがあれば、消してかまいません。同じ名前のアイコンが2つ並ぶと、どちらを開いたか分かりにくくなります。

アプリを入れずにホーム画面に追加して使うときは、「Webアプリとして開く」のスイッチをオフにして追加します。オンのままだと、LINE でログインできません。

</HelpStep>

</HelpSteps>
