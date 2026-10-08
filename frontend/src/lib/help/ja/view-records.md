<script>
	import HelpSteps from '$lib/components/help-steps.svelte';
	import HelpStep from '$lib/components/help-step.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<HelpSteps>

<HelpStep n={1}>

「{m.period_nav_week_button()}」か「{m.period_nav_month_button()}」を押して、見る長さを選びます。

</HelpStep>

<HelpStep n={2}>

左右の矢印で、前や次の期間に動きます。まん中の日付を押すと、今の期間に戻ります。

</HelpStep>

<HelpStep n={3}>

グラフで、血圧の変わり方を見ます。線の名前を押すと、その線を出したり隠したりできます。

</HelpStep>

<HelpStep n={4}>

朝と夜の平均で、期間の中の血圧をまとめて見ます。朝・夜の時間帯の外 (昼など) に測った記録は、平均とグラフには入りません。

</HelpStep>

</HelpSteps>
