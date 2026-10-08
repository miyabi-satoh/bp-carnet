<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import HelpExternalLink from '$lib/components/help-external-link.svelte';
	import { APP_STORE_URL } from '$lib/app-store-link';
</script>

<!-- App Store とホーム画面の操作はアプリの外なので画像を撮れない。 -->

<HelpSteps>

<HelpStep n={1} image={false}>

App Store から BP Carnet のアプリを入れます。

<HelpExternalLink href={APP_STORE_URL}>App Store で見る</HelpExternalLink>

</HelpStep>

<HelpStep n={2} image={false}>

アプリを開き、ブラウザで使っていたのと同じ方法でログインします。記録も、買い足した読み取りの枠も、そのまま使えます。

</HelpStep>

<HelpStep n={3} image={false}>

Safari から「ホーム画面に追加」したアイコンがあれば、消してかまいません。同じ名前のアイコンが2つ並ぶと、どちらを開いたか分かりにくくなります。

</HelpStep>

</HelpSteps>
