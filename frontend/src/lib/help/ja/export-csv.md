<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

すべての記録を CSV ファイルに保存します。Excel などで開いたり、控えとして取っておいたりできます。

<HelpSteps>

<HelpStep n={1}>

画面の右上の丸いボタンを押し、「{m.settings_title()}」を押します。

</HelpStep>

<HelpStep n={2}>

「{m.settings_export_csv_button()}」を押すと、すべての記録がファイルに保存されます。

</HelpStep>

<!-- 保存したファイルはアプリの外で開くので、画面の画像を撮れない。 -->

<HelpStep n={3} image={false}>

ファイルの名前は「bp-records-日付.csv」です。Excel などで開けます。

</HelpStep>

</HelpSteps>
