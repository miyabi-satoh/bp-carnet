<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { fetchOcrTopupResult } from '$lib/payments';
	import { LatestRequest } from '$lib/latest-request';
	import ButtonRow from '$lib/components/button-row.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';

	/** Stripe Checkout の成功URLの戻り先。
	 * config の checkout_success_url をこのルートに向け、`session_id` を付ける。webhook 完了前に
	 * 戻ることがあるため、確定するまで結果APIを短時間ポーリングする。ここでは枠を付与しない。 */

	// success_url の `?session_id={CHECKOUT_SESSION_ID}` を Stripe が実IDに置換して戻す。
	const sessionId = page.url.searchParams.get('session_id');

	type Phase = 'processing' | 'succeeded' | 'failed' | 'error';
	let phase = $state<Phase>(sessionId === null ? 'error' : 'processing');
	/** ポーリングが尽きても processing のまま (webhook が遅れている)。確かめ直す・戻る導線を出す。 */
	let timedOut = $state(false);

	const POLL_INTERVAL_MS = 2000;
	const MAX_ATTEMPTS = 15; // 約30秒
	let attempts = 0;
	let timer: ReturnType<typeof setTimeout> | null = null;
	const pollingRequest = new LatestRequest();

	async function poll(isLatest: () => boolean) {
		if (sessionId === null) return;
		const result = await fetchOcrTopupResult(sessionId);
		if (!isLatest()) return;
		if (!result.ok) {
			// 本人のSessionでない・存在しない (404) は確定した失敗として扱う。通信エラー等の
			// 一時的な失敗は、ポーリングを続ける (webhook 完了待ちの最中に切らないため)。
			if (result.code === 'not_found') {
				phase = 'error';
				return;
			}
		} else if (result.data.status !== 'processing') {
			phase = result.data.status;
			return;
		}
		attempts += 1;
		if (attempts >= MAX_ATTEMPTS) {
			timedOut = true;
			return;
		}
		timer = setTimeout(() => void poll(isLatest), POLL_INTERVAL_MS);
	}

	onMount(() => {
		if (sessionId !== null) void poll(pollingRequest.begin());
	});
	onDestroy(() => {
		pollingRequest.cancel();
		if (timer) clearTimeout(timer);
	});

	/** 打ち切った後に、同じ Checkout Session で確かめ直す。Webhook が遅れて届いていれば成功に切り替わる
	 * (確かめられないまま戻って、もう一度買ってしまわないため)。 */
	function checkAgain() {
		attempts = 0;
		timedOut = false;
		void poll(pollingRequest.begin());
	}

	function backToPhoto() {
		void goto(resolve('/record/photo'));
	}
</script>

<!-- ほかの画面と同じく、見出しを上に置き、結果をカード1枚に出す。
     Stripe から戻ってくる画面なので、戻るボタンは置かず、行き先はカードの中のボタンで示す。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<!-- 見出しの高さは PageHeader (丸い戻るボタンの行) にそろえる。 -->
	<div class="pt-safe-5 pb-5">
		<h1 class="flex min-h-11 items-center text-lg font-extrabold">{m.topup_result_title()}</h1>
	</div>

	<div class="flex flex-col gap-4 rounded-lg raised bg-card p-6">
		{#if phase === 'processing'}
			{#if timedOut}
				<p class="text-sm text-muted-foreground">{m.topup_result_timed_out_description()}</p>
				<ButtonRow>
					<Button type="button" variant="outline" onclick={backToPhoto}>
						{m.topup_result_back_to_photo_button()}
					</Button>
					<Button type="button" onclick={checkAgain}>
						{m.topup_result_check_again_button()}
					</Button>
				</ButtonRow>
			{:else}
				<LoadingIndicator class="justify-start py-0" label={m.topup_result_processing_title()} />
				<p class="text-sm text-muted-foreground">{m.topup_result_processing_description()}</p>
			{/if}
		{:else if phase === 'succeeded'}
			<div
				class="flex size-13 items-center justify-center rounded-full bg-primary/10 text-primary"
				aria-hidden="true"
			>
				<CheckIcon size={26} strokeWidth={2.6} />
			</div>
			<div>
				<PageResult
					title={m.topup_result_succeeded_title()}
					description={m.topup_result_succeeded_description()}
				/>
			</div>
			<Button type="button" class="w-full" onclick={backToPhoto}>
				{m.topup_result_back_to_photo_button()}
			</Button>
		{:else if phase === 'failed'}
			<div
				class="flex size-13 items-center justify-center rounded-full bg-destructive/10 text-destructive"
				aria-hidden="true"
			>
				<XIcon size={26} strokeWidth={2.6} />
			</div>
			<div>
				<PageResult
					title={m.topup_result_failed_title()}
					description={m.topup_result_failed_description()}
				/>
			</div>
			<!-- デザインは縦に積むが、スマートフォンの幅に収まるボタンの組は横に並べる。 -->
			<ButtonRow>
				<Button type="button" variant="outline" onclick={backToPhoto}>
					{m.topup_result_back_to_photo_button()}
				</Button>
				<Button type="button" onclick={backToPhoto}>
					{m.topup_result_retry_button()}
				</Button>
			</ButtonRow>
		{:else}
			<p class="text-sm text-muted-foreground">{m.topup_result_error_description()}</p>
			<Button type="button" class="w-full" onclick={backToPhoto}>
				{m.topup_result_back_to_photo_button()}
			</Button>
		{/if}
	</div>
</div>
