<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

「{m.photo_page_retake_button()}」を押して、撮り直した写真を選びます。光が映り込まないように、数字を大きく写します。

</HelpStep>

<HelpStep n={2}>

「{m.photo_page_quota_exhausted_title()}」と出たら、写真では読み取れません。次の手順で数字を入れます。

</HelpStep>

<HelpStep n={3}>

読み取れないときや、「{m.home_photo_ocr_button()}」が無いとき (写真での記録を使えない設定) は、「{m.home_add_record_button()}」から数字を入れます。

</HelpStep>

</HelpSteps>
