<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

画面の下の「{m.home_add_record_button()}」を押します。

</HelpStep>

<HelpStep n={2}>

上の血圧・下の血圧・脈拍を入れます。測った日時が違うときは直します。

</HelpStep>

<HelpStep n={3}>

「{m.record_form_dialog_create_button()}」を押します。

</HelpStep>

<HelpStep n={4}>

記録一覧に出たら、記録できています。

</HelpStep>

</HelpSteps>
