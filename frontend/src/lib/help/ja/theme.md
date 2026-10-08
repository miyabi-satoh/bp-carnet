<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

画面の右上の、太陽か月の絵のボタンを押します。

</HelpStep>

<HelpStep n={2}>

「{m.theme_light()}」か「{m.theme_dark()}」を押すと、その明るさになります。

</HelpStep>

<HelpStep n={3}>

「{m.theme_system()}」を押すと、スマホの明るさの設定に合わせて変わります。

</HelpStep>

</HelpSteps>
