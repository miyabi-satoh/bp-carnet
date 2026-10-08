<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

ログイン画面の「{m.login_signup_link()}」を押します。

</HelpStep>

<HelpStep n={2}>

メールアドレスを入れて、「{m.signup_submit_button()}」を押します。

</HelpStep>

<HelpStep n={3}>

届いたメールのリンクを開きます。パスワードを入れて、「{m.verify_email_submit_button()}」を押します。

</HelpStep>

</HelpSteps>
