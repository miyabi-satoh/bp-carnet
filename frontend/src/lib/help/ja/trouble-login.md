<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

パスワードの欄の、目の絵のボタンを押すと、入れた文字を確かめられます。

</HelpStep>

<HelpStep n={2}>

パスワードを忘れたときは、「{m.login_forgot_password_link()}」を押します。

</HelpStep>

<HelpStep n={3}>

登録したメールアドレスを入れて、「{m.reset_password_request_submit_button()}」を押します。届いたメールのリンクから、新しいパスワードを決めます。

</HelpStep>

</HelpSteps>
