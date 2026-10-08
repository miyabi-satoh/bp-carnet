<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

画面の右上の丸いボタンを押し、「{m.settings_title()}」を押します。

</HelpStep>

<HelpStep n={2}>

記録の時刻がずれているときは、「{m.settings_timezone_section_title()}」の欄を押します。

</HelpStep>

<HelpStep n={3}>

先頭の「自動設定」を選ぶと、使っている端末の地域に合わせます。ほかの地域にするときは、その地域を選びます。選ぶとすぐに保存されます。

</HelpStep>

</HelpSteps>
