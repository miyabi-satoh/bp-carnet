<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import HelpExternalLink from '$lib/components/help-external-link.svelte';
</script>

<!-- ブラウザのメニューはアプリの外なので画像を撮れない。表記はブラウザの版で変わるため、細かい操作は公式の手順 (Apple サポート・Chrome ヘルプ) に任せる。 -->

<HelpSteps>

<HelpStep n={1} image={false}>

Chrome でこのアプリを開きます。

</HelpStep>

<HelpStep n={2} image={false}>

アドレスバーの右のメニューを押し、「インストール」の項目を選びます。

<HelpExternalLink href="https://support.google.com/chrome/answer/9658361?hl=ja&co=GENIE.Platform%3DAndroid">詳しい手順 (Google のサイト)</HelpExternalLink>

</HelpStep>

<HelpStep n={3} image={false}>

ホーム画面にできたアイコンを押すと、アプリが開きます。

</HelpStep>

</HelpSteps>
