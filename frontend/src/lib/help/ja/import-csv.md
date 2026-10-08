<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

このアプリで書き出した形の CSV ファイルから、記録をまとめて取り込みます。取り込む日の記録は、ファイルの内容に置き換わります。

<HelpSteps>

<HelpStep n={1}>

画面の右上の丸いボタンを押し、「{m.settings_title()}」を押します。

</HelpStep>

<HelpStep n={2}>

「{m.settings_import_button()}」を押し、取り込む CSV ファイルを選びます。

</HelpStep>

<HelpStep n={3}>

読み込んだ記録を確かめて、「{m.common_review_button()}」を押します。

</HelpStep>

<HelpStep n={4}>

取り込む内容を確かめて、「{m.import_preview_confirm_button()}」を押します。取り込む日のもとの記録は置き換わり、「{m.import_preview_status_removed()}」と出た記録は消えます。

</HelpStep>

</HelpSteps>
