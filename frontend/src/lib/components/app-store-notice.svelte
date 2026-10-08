<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import { APP_STORE_URL } from '$lib/app-store-link';
	import { isIosWebApp } from '$lib/ios-web-app';
	import { Button } from '$lib/components/ui/button';
	import XIcon from '@lucide/svelte/icons/x';

	const DISMISSED_KEY = 'app-store-notice-dismissed';

	/** 閉じたことは、この端末に覚える。出すかどうかが端末 (ホーム画面のウェブアプリか) で決まる案内のため。 */
	function readDismissed(): boolean {
		try {
			return localStorage.getItem(DISMISSED_KEY) === '1';
		} catch {
			return false;
		}
	}

	// Smart App Banner は Safari の画面の機能で、ホーム画面から全画面で開いたウェブアプリに出るとは
	// Apple の資料に書かれていない。出なくてもアプリへ移れるよう、ここで案内する。
	let visible = $state(isIosWebApp() && !readDismissed());

	function dismiss() {
		visible = false;
		try {
			localStorage.setItem(DISMISSED_KEY, '1');
		} catch {
			// 覚えられなくても、この表示の間は閉じたままにする。
		}
	}
</script>

{#if visible}
	<section
		aria-labelledby="app-store-notice-title"
		class="flex items-start gap-2 rounded-md border bg-card py-3 pr-1 pl-4"
	>
		<div class="flex flex-1 flex-col gap-2">
			<h2 id="app-store-notice-title" class="text-sm font-bold">{m.app_store_notice_title()}</h2>
			<p class="text-sm leading-relaxed text-muted-foreground">
				{m.app_store_notice_description()}
			</p>
			<!-- App Store のページは外のサイト。SvelteKit のルートではないので resolve() は使わない。 -->
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<Button href={APP_STORE_URL} variant="outline" class="self-start">
				{m.app_store_notice_link()}
			</Button>
		</div>
		<Button
			type="button"
			variant="ghost"
			size="icon"
			aria-label={m.common_close_button()}
			onclick={dismiss}
		>
			<XIcon />
		</Button>
	</section>
{/if}
