<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

印刷したい期間を選んで、「{m.home_report_button()}」を押します。

</HelpStep>

<HelpStep n={2}>

期間を確かめます。違うときは、ここで選び直します。

</HelpStep>

<HelpStep n={3}>

「{m.report_print_button()}」を押すと、印刷の画面が出ます。

</HelpStep>

</HelpSteps>
