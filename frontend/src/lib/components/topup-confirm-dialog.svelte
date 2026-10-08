<script lang="ts">
	import { resolve } from '$app/paths';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import { busyCloseGuard } from '$lib/dialog';
	import { navigateTo } from '$lib/external-navigation';
	import { startOcrTopupCheckout } from '$lib/payments';
	import { purchaseTopup, topupDisplayPrice } from '$lib/app-store-purchase';
	import { fetchContactUrl } from '$lib/auth';
	import { isNativeApp } from '$lib/native-app';
	import { isTaxIncluded } from '$lib/tax';
	import * as m from '$lib/paraglide/messages.js';
	import NewTabLink from '$lib/components/new-tab-link.svelte';

	/** 写真の読み取りを買い足す前の確認ダイアログ。
	 * 写真で記録ページ・設定の買い足しボタンから `bind:this` + `show()` で開く。「同意して購入へ進む」で
	 * Checkout を作り、Stripe の URL へ遷移する。Stripe との往復を待つ間は、ほかの非同期の確認
	 * (`unlink-identity-dialog.svelte` 等) と同じく、ボタンに送信待ちを出して閉じさせない。
	 * アプリではアプリ内課金の購入シートを出し、終わったら結果を知らせる (docs/payments.md)。 */

	type Props = {
		/** アプリで枠を足したとき。枠の表示を読み直す。 */
		onpurchased?: () => void;
	};

	let { onpurchased }: Props = $props();

	// 表示する金額 (→ docs/payments.md)。Stripe の Price と一致させる。
	const TOPUP_PRICE_YEN = 300;
	/** 価格に添える消費税の書き方。インボイスの登録の日から税込み (→ docs/payments.md)。
	 * 開いたまま日をまたいでも合うよう、開くたびに決める。 */
	let taxLabel = $state('');

	let open = $state(false);
	/** Checkout の作成から遷移するまで (アプリでは購入が終わるまで)。二重送信・遷移中の再クリックを防ぐ。 */
	let starting = $state(false);
	const closeGuard = busyCloseGuard(() => starting);
	let noticeDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	/** 問い合わせ先 (`[server] contact_url`)。アプリで開いたときに取る。 */
	let contactUrl = $state<string | null>(null);
	/** アプリの価格 (StoreKit の表示用の価格)。読み込み中は `undefined`、取れなければ `null` (買えない)。 */
	let appPrice = $state<string | null | undefined>(undefined);

	export function show() {
		taxLabel = isTaxIncluded() ? m.topup_confirm_tax_included() : m.topup_confirm_tax_exempt();
		open = true;
		if (isNativeApp && !appPrice) {
			appPrice = undefined;
			// 問い合わせ先は、反映できなかった購入の知らせに使う。購入の後に取ると、待つ間に買い足しを開き直せたり、
			// 知らせの文言が出し直しで変わったりするので、価格と一緒に取り終えてから買えるようにする。
			void Promise.all([topupDisplayPrice(), fetchContactUrl()]).then(([price, url]) => {
				contactUrl = url;
				appPrice = price;
			});
		}
	}

	async function agree(event: SubmitEvent) {
		event.preventDefault();
		if (starting) return;
		starting = true;
		if (isNativeApp) {
			await purchaseInApp();
			return;
		}
		const result = await startOcrTopupCheckout();
		if (!result.ok) {
			starting = false;
			open = false;
			noticeDialog?.show(m.topup_confirm_title(), result.message);
			return;
		}
		// Stripe-hosted Checkout は外部 URL なので SPA 遷移 (goto) ではなくフルナビゲーション。
		// 遷移するまで starting は下ろさない (戻る操作で二重に押されないように)。
		navigateTo(result.data.checkoutUrl);
	}

	async function purchaseInApp() {
		let outcome: Awaited<ReturnType<typeof purchaseTopup>>;
		try {
			outcome = await purchaseTopup();
		} finally {
			starting = false;
		}
		if (outcome.kind === 'cancelled') return;
		open = false;
		switch (outcome.kind) {
			case 'granted':
				onpurchased?.();
				noticeDialog?.show(
					m.topup_confirm_app_succeeded_title(),
					m.topup_confirm_app_succeeded_description()
				);
				break;
			case 'deferred':
				noticeDialog?.show(m.topup_confirm_title(), m.topup_confirm_app_deferred_description());
				break;
			case 'pending':
				noticeDialog?.show(m.topup_confirm_title(), m.topup_confirm_app_pending_description());
				break;
			case 'rejected':
				noticeDialog?.show(m.topup_confirm_title(), m.topup_confirm_app_rejected_description(), {
					contact: true
				});
				break;
			case 'failed':
				noticeDialog?.show(m.topup_confirm_title(), outcome.message);
				break;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content {...closeGuard}>
		<Dialog.Header>
			<Dialog.Title>{m.topup_confirm_title()}</Dialog.Title>
			<Dialog.Description class="sr-only">{m.topup_confirm_title()}</Dialog.Description>
		</Dialog.Header>

		<!-- 価格。アプリは App Store の価格 (売り手は Apple で、税込み) をそのまま出す。 -->
		<div class="flex items-baseline gap-1 rounded-md bg-muted px-4 py-3">
			{#if !isNativeApp}
				<span class="text-3xl font-bold text-primary">{TOPUP_PRICE_YEN}</span>
				<span class="text-base font-bold text-primary">{m.topup_confirm_yen()}</span>
				<span class="ml-1 text-xs text-muted-foreground">（{taxLabel}）</span>
			{:else if appPrice}
				<span class="text-3xl font-bold text-primary">{appPrice}</span>
			{:else if appPrice === null}
				<span class="text-sm text-muted-foreground">{m.topup_confirm_app_unavailable()}</span>
			{:else}
				<span class="text-sm text-muted-foreground">{m.topup_confirm_app_price_loading()}</span>
			{/if}
		</div>

		<!-- 事前に分かるべき条件 -->
		<ul class="flex flex-col gap-2 text-sm text-muted-foreground">
			<li>{m.topup_confirm_term_amount()}</li>
			<li>
				{isNativeApp ? m.topup_confirm_term_refund_apple() : m.topup_confirm_term_no_refund()}
			</li>
			<li>{m.topup_confirm_term_delete()}</li>
		</ul>

		<!-- ボタンの上の余白は DialogFormFooter が持つので、フォームで包むのはリンクとボタンだけ。 -->
		<form onsubmit={agree}>
			<!-- 規約類 (別タブで開く。ダイアログを離れずに読めるように)。 -->
			<p class="text-xs leading-relaxed text-muted-foreground">
				<NewTabLink href={resolve('/terms')}>{m.terms_title()}</NewTabLink
				>{m.common_list_separator()}<NewTabLink href={resolve('/privacy')}
					>{m.privacy_title()}</NewTabLink
				>{m.common_list_separator()}<NewTabLink href={resolve('/tokushoho')}
					>{m.tokushoho_title()}</NewTabLink
				>
			</p>

			<DialogFormFooter loading={starting} oncancel={() => (open = false)}>
				<LoadingButton type="submit" loading={starting} disabled={isNativeApp && !appPrice}>
					{m.topup_confirm_agree_button()}
				</LoadingButton>
			</DialogFormFooter>
		</form>
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={noticeDialog} {contactUrl} />
