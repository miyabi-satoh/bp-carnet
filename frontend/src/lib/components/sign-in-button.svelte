<script lang="ts">
	import type { LinkedProvider } from '$lib/auth';
	import googleGLogo from '$lib/assets/google-g-logo.svg';
	import lineLoginIcon from '$lib/assets/line-login-icon.png';

	/** LINE・Google・Apple でのログインを始めるボタン (backend のログインの開始へのリンク)。
	 * 各社のガイドラインを守ったうえで骨組みをそろえる (docs/authentication.md)。
	 * `onclick` を渡すとリンクではなくボタンになる (アプリは各社の SDK でログインするため、docs/mobile-app.md)。 */
	let {
		provider,
		label,
		onclick,
		disabled = false
	}: {
		provider: LinkedProvider;
		label: string;
		onclick?: () => void;
		disabled?: boolean;
	} = $props();

	const classes = $derived([
		'relative isolate flex h-11 w-full items-center overflow-hidden rounded-md border font-sign-in text-sm font-medium before:pointer-events-none before:absolute before:inset-0 before:bg-black/0 focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none',
		provider === 'line'
			? // LINE: マウスオーバーは黒 10%、押下は黒 30%。
				'border-transparent bg-line-button pl-1.5 text-white hover:before:bg-black/10 active:before:bg-black/30'
			: provider === 'google'
				? // Google: ガイドラインの light・dark の色と、それぞれのマウスオーバー・押下の重ね。
					'border-google-button-border bg-google-button pl-2.5 text-google-button-foreground hover:before:bg-black/5 active:before:bg-black/10 dark:hover:before:bg-white/8 dark:active:before:bg-white/12'
				: // Apple: HIG により、明るい背景では黒、暗い背景では白。ロゴと文言は地と逆の黒か白だけ。
					'border-transparent bg-apple-button pl-2.5 text-apple-button-foreground hover:before:bg-white/15 active:before:bg-white/25 dark:hover:before:bg-black/5 dark:active:before:bg-black/10'
	]);
</script>

<!-- 高さ 44px (Google の公式素材 40px の 1.1 倍)、アイコンは左端から約 11px、文言は残りの幅の中央、
     文言のフォントは Google Sans Medium 14px。
     マウスオーバー・押下の色は、基本色の上、アイコン・縦線・文言の下に重ねる (::before を先に描き、中身を relative で上に出す)。 -->
{#snippet content()}
	{#if provider === 'line'}
		<!-- LINE ログインボタンのテンプレートのアイコン (line_132.png)。ガイドラインにより改変しない。
		     30px で吹き出しが約 22px になり、Google の G (22px) と見た目の大きさがそろう。
		     アイコンと文言の間に縦線、文言の左右の余白は吹き出しの幅 (アイコンの約 74%) 以上。 -->
		<img src={lineLoginIcon} alt="" width="30" height="30" class="relative size-7.5 shrink-0" />
		<span class="relative h-full w-px bg-black/8" aria-hidden="true"></span>
		<span class="relative flex-1 px-6 text-center">{label}</span>
	{:else if provider === 'apple'}
		<!-- Apple のロゴ。形は Apple Design Resources の Sign in with Apple (Left-aligned・Medium・44pt) と同じで、
		     高さ 44px のボタンでロゴが 19px になる比率を保つ。Google の G と同じ 22px の枠の中央に置き、
		     色は文言と同じ (HIG により黒か白だけ)。 -->
		<svg
			viewBox="4.48 9 22 22"
			width="22"
			height="22"
			class="relative size-5.5 shrink-0"
			aria-hidden="true"
		>
			<path
				fill="currentColor"
				d="M15.71 14.885c.858 0 1.933-.58 2.573-1.353.58-.7 1.002-1.679 1.002-2.657 0-.133-.012-.266-.036-.375-.954.036-2.102.64-2.79 1.45-.544.616-1.039 1.582-1.039 2.572 0 .145.024.29.036.339.06.012.157.024.254.024ZM12.69 29.5c1.172 0 1.691-.785 3.153-.785 1.486 0 1.812.76 3.116.76 1.28 0 2.138-1.183 2.947-2.342.906-1.329 1.28-2.634 1.305-2.694-.085-.024-2.537-1.027-2.537-3.841 0-2.44 1.933-3.539 2.042-3.624-1.28-1.836-3.225-1.884-3.757-1.884-1.437 0-2.609.87-3.346.87-.797 0-1.848-.822-3.092-.822-2.367 0-4.771 1.957-4.771 5.653 0 2.295.894 4.723 1.993 6.293.942 1.329 1.764 2.416 2.947 2.416Z"
			/>
		</svg>
		<span class="relative ml-2.5 flex-1 pr-3 text-center">{label}</span>
	{:else}
		<!-- Sign in with Google の公式素材 (signin-assets.zip の Light・Show text=No・Square・Android+Web の SVG) から、
		     ボタンの地と枠線の2つの図形だけを外したもの。ガイドラインにより、ロゴの部分は改変しない。
		     G は公式素材の比率 (40px のボタンに 20px の G を左から 10px) を 1.1 倍して、22px を左から 11px に置く。
		     G の後 10px・文言の後 12px の余白を取る。 -->
		<img src={googleGLogo} alt="" width="22" height="22" class="relative size-5.5 shrink-0" />
		<span class="relative ml-2.5 flex-1 pr-3 text-center">{label}</span>
	{/if}
{/snippet}

{#if onclick}
	<button type="button" class={[classes, 'disabled:opacity-50']} {onclick} {disabled}>
		{@render content()}
	</button>
{:else}
	<!-- backend の API エンドポイントへのフルナビゲーション (OAuth のリダイレクトフローが必要なため、
	     data-sveltekit-reload で SvelteKit のクライアントルーターに横取りさせない)。
	     SvelteKit のルートではなく backend の API パスなので resolve() は使わない。 -->
	<!-- eslint-disable svelte/no-navigation-without-resolve -->
	<a href="/api/v1/auth/{provider}/login" data-sveltekit-reload class={classes}>
		{@render content()}
	</a>
	<!-- eslint-enable svelte/no-navigation-without-resolve -->
{/if}
