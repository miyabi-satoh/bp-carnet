<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

直したい記録の、鉛筆のボタンを押します。

</HelpStep>

<HelpStep n={2}>

数字を直して、「{m.record_form_dialog_update_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

消すときは、消したい記録のゴミ箱のボタンを押します。

</HelpStep>

<HelpStep n={4}>

確かめて、「{m.common_delete_button()}」を押します。消した記録は元に戻せません。

</HelpStep>

</HelpSteps>
