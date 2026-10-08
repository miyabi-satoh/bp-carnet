<script lang="ts">
	import type MailIcon from '@lucide/svelte/icons/mail';

	type Props = {
		/** 見出しの上に置く lucide のアイコン。 */
		icon: typeof MailIcon;
		title: string;
		subtitle: string;
	};

	let { icon: Icon, title, subtitle }: Props = $props();
</script>

<!-- 未ログインで開く画面 (サインアップ・パスワードの設定と再設定) と、通信できないときの画面の見出し。
     パスワード再設定の画面のデザインに揃える (ログイン画面はその画面のデザインで別に組む)。 -->
<div class="mb-6 w-full max-w-80 text-center">
	<div
		class="mx-auto mb-4 flex size-16 items-center justify-center rounded-full bg-secondary text-secondary-foreground"
	>
		<Icon class="size-7" aria-hidden="true" />
	</div>
	<h1 class="text-xl font-extrabold">{title}</h1>
	<p class="mt-1.5 text-sm text-muted-foreground">{subtitle}</p>
</div>
