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

いちばん下の「{m.common_delete_account_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

説明を読んで、欄に「{m.delete_account_dialog_confirm_word()}」と入れます。

</HelpStep>

<HelpStep n={4}>

「{m.delete_account_dialog_submit_button()}」を押します。

</HelpStep>

<HelpStep n={5}>

30日たつと、記録と一緒に消えます。それまでにログアウトしてログインし直すと、削除は取り消されます。消える日は、設定画面のいちばん下に出ます。

</HelpStep>

</HelpSteps>
