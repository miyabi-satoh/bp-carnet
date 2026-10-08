<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

画面の下の「{m.home_photo_ocr_button()}」を押し、手帳のページを撮るか、撮った写真を選びます。

</HelpStep>

<HelpStep n={2}>

写真を確かめて、「{m.photo_page_read_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

読み取った記録を確かめます。数字が違う記録は、鉛筆のボタンから直します。チェックの付いた記録を取り込みます。

</HelpStep>

<HelpStep n={4}>

「{m.common_review_button()}」を押します。

</HelpStep>

<HelpStep n={5}>

取り込む内容を確かめて、「{m.import_preview_confirm_button()}」を押します。取り込む日のもとの記録は置き換わり、「{m.import_preview_status_removed()}」と出た記録は消えます。

</HelpStep>

</HelpSteps>
