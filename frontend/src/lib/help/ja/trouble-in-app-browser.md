<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<!-- LINE などアプリの外の操作 (1・2歩目) は画像を撮れないため、文章だけにする。 -->

<HelpSteps>

<HelpStep n={1} image={false}>

LINE やメールのリンクを押すと、アプリの中の簡易な画面（内蔵ブラウザ）で開くことがあります。この画面では、LINE・Google・Apple でログインできません。

</HelpStep>

<HelpStep n={2} image={false}>

画面のメニュー（︙ や共有のマーク）から「ブラウザで開く」（iPhone は「Safari で開く」）を選びます。

</HelpStep>

<HelpStep n={3}>

メニューが見つからないときは、ログイン画面の「{m.login_in_app_browser_copy_button()}」を押し、Safari や Chrome のアドレス欄に貼り付けて開きます。

</HelpStep>

</HelpSteps>
