<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

朝か夜かは、測った時刻で自動で決まります (はじめは朝 4:00〜10:00、夜 18:00〜24:00)。平均とグラフには、この時間帯の記録だけを使います。起きる時間や寝る時間に合わせて変えてください。変えると、これまでの記録の分け方も変わります。

<HelpSteps>

<HelpStep n={1}>

画面の右上の丸いボタンを押し、「{m.settings_title()}」を押します。

</HelpStep>

<HelpStep n={2}>

「{m.settings_period_section_title()}」で、変えたい時刻を押します。

</HelpStep>

<HelpStep n={3}>

時と分を選んで、「{m.common_close_button()}」を押します。選んだ時刻はすぐに保存されます。

</HelpStep>

</HelpSteps>
