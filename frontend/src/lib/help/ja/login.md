<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

LINE・Google・Apple のボタンのどれかを押して進めます。初めてのときは、そのままアカウントができます。

</HelpStep>

<HelpStep n={2}>

メールアドレスで作ったアカウントは、{m.common_email_label()}と{m.common_password_label()}を入れて「{m.login_submit_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

記録の画面が出たら、ログインできています。

</HelpStep>

</HelpSteps>
