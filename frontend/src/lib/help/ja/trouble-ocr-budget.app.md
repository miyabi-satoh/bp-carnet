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

<!-- 2〜4 はアプリだけの画面で、画像を撮る E2E (ウェブ) では出せないため、画像を置かない。
     1 はウェブと同じ画面なので、ウェブの画像を使う。 -->

<HelpStep n={2} image={false}>

値段と条件を確かめて、「{m.topup_confirm_agree_button()}」を押します。返金は Apple の定めによります。

</HelpStep>

<HelpStep n={3} image={false}>

Apple の購入の画面が出たら、画面の案内に従って購入します。

</HelpStep>

<HelpStep n={4} image={false}>

「{m.topup_confirm_app_succeeded_title()}」と出たら、「{m.common_close_button()}」を押します。選んだ写真を「{m.photo_page_read_button()}」で読み取れます。

</HelpStep>

</HelpSteps>
