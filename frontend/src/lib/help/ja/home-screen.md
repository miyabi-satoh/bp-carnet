<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import HelpExternalLink from '$lib/components/help-external-link.svelte';
</script>

<!-- ブラウザのメニューはアプリの外なので画像を撮れない。表記はブラウザの版で変わるため、細かい操作は公式の手順 (Apple サポート・Chrome ヘルプ) に任せる。 -->

<HelpSteps>

<HelpStep n={1} image={false}>

Safari でこのアプリを開きます。共有のボタンを押し、「ホーム画面に追加」を選びます。

<HelpExternalLink href="https://support.apple.com/ja-jp/guide/iphone/iphea86e5236/ios">詳しい手順 (Apple のサイト)</HelpExternalLink>

</HelpStep>

<HelpStep n={2} image={false}>

「Webアプリとして開く」のスイッチが出たら、オフにします。オンのままだと、LINE でログインできません。

</HelpStep>

<HelpStep n={3} image={false}>

ホーム画面にできたアイコンを押すと、アプリが開きます。

</HelpStep>

</HelpSteps>
