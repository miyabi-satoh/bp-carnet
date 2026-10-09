<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

写真の読み取りには無料の枠があります（血圧計の写真で約60回、手帳のページで約13ページが目安です。写真によって前後します）。使い切っても、数字を入れての記録はこれまでどおりできます。写真で記録し続けたいときは、読み取りを購入します。

<HelpSteps>

<HelpStep n={1}>

「{m.home_photo_ocr_button()}」で写真を選ぶと、使い切ったことが出ます。「{m.photo_page_topup_button()}」を押します。

</HelpStep>

<HelpStep n={2}>

値段と条件を確かめて、「{m.topup_confirm_agree_button()}」を押します。

</HelpStep>

<!-- Stripe の画面はアプリの外なので、画像を撮らない。 -->

<HelpStep n={3} image={false}>

支払いの画面 (Stripe) で、支払い方を選んで支払います。

</HelpStep>

<HelpStep n={4}>

「{m.topup_result_succeeded_title()}」と出たら、「{m.topup_result_back_to_photo_button()}」を押して、写真を選び直します。

</HelpStep>

</HelpSteps>
