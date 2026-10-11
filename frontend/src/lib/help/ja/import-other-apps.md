<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

他の血圧記録アプリや血圧計のアプリで書き出したファイルから、記録をまとめて取り込みます。取り込む日の記録は、ファイルの内容に置き換わります。

読み込めるのは、CSV などの表の形のファイル、PDF ファイル、iPhone の「ヘルスケア」のアプリで書き出したデータ（zip ファイル）です。

CSV と PDF のファイルは、AI（Google の Gemini API）で読み取ります。写真の読み取りと同じ、読み取りの枠を使います。「ヘルスケア」のデータは、AI を使わずに読み込みます。

<HelpSteps>

<HelpStep n={1}>

画面の右上の丸いボタンを押し、「{m.settings_title()}」を押します。

</HelpStep>

<HelpStep n={2}>

「{m.settings_import_button()}」を押し、他のアプリで書き出したファイルを選びます。

</HelpStep>

<HelpStep n={3}>

CSV と PDF のファイルでは、AI に送るものの確認が出ます。内容を確かめて、「{m.file_read_consent_agree_button()}」を押します。「ヘルスケア」のデータでは、この確認は出ません。

</HelpStep>

<HelpStep n={4}>

読み込んだ記録を確かめて、「{m.common_review_button()}」を押します。「{m.import_rows_needs_review_badge()}」と出た記録は、鉛筆のボタンから直せます。

</HelpStep>

<HelpStep n={5}>

取り込む内容を確かめて、「{m.import_preview_confirm_button()}」を押します。取り込む日のもとの記録は置き換わり、「{m.import_preview_status_removed()}」と出た記録は消えます。

</HelpStep>

</HelpSteps>
