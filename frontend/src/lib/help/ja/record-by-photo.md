<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

画面の下の「{m.home_photo_ocr_button()}」を押し、血圧計の数字を撮るか、撮った写真を選びます。

</HelpStep>

<HelpStep n={2}>

写真を確かめて、「{m.photo_page_read_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

読み取った数字を確かめます。違っていたら直します。

</HelpStep>

<HelpStep n={4}>

測った日時を確かめて、「{m.record_form_dialog_create_button()}」を押します。

</HelpStep>

</HelpSteps>
