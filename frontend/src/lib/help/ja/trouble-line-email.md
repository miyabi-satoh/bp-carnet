<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1} image={false}>

ログインでは、LINE に登録したメールアドレスを使います。LINE の「設定」→「アカウント」でメールアドレスを登録します。

</HelpStep>

<HelpStep n={2}>

ログイン画面で「{m.login_line_button()}」を押します。

</HelpStep>

<HelpStep n={3} image={false}>

LINE の同意の画面で、メールアドレスを提供するスイッチをオンにして進めます。

</HelpStep>

</HelpSteps>
