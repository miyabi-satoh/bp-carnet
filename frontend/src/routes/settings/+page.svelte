<script lang="ts">
	import { goto, invalidateAll } from '$app/navigation';
	import { onDestroy, onMount, tick } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import {
		canUnlinkIdentity,
		fetchContactUrl,
		userTimeZone,
		LINKED_PROVIDERS,
		linkedProviderLabel
	} from '$lib/auth';
	import * as m from '$lib/paraglide/messages.js';
	import {
		autoTimezoneLabel,
		browserTimeZone,
		fetchSettings,
		fetchTimezones,
		periodThresholdErrors,
		periodThresholdsOf,
		saveSettings,
		timezoneChoiceOptions,
		type PeriodThresholds,
		type Settings
	} from '$lib/settings';
	import { formatDateLabel, formatFutureDateLabel, type DayPeriod } from '$lib/period';
	import { LIST_ROW_CLASS, SECTION_TITLE_CLASS } from '$lib/styles';
	import { cn, openFilePicker } from '$lib/utils';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { Spinner } from '$lib/components/ui/spinner';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LogoutDialog from '$lib/components/logout-dialog.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import DeleteAccountDialog from '$lib/components/delete-account-dialog.svelte';
	import UnlinkIdentityDialog from '$lib/components/unlink-identity-dialog.svelte';
	import TimePicker from '$lib/components/time-picker.svelte';
	import OcrQuotaGauge from '$lib/components/ocr-quota-gauge.svelte';
	import TopupConfirmDialog from '$lib/components/topup-confirm-dialog.svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import { exportRecords } from '$lib/export';
	import { appVersionLabel, isNativeApp } from '$lib/native-app';
	import { CSV_FILE_ACCEPT, importFileHandoff } from '$lib/import';
	import { takeImportResult } from '$lib/import-step';
	import { withdrawOcrConsent } from '$lib/ocr';
	import { OcrStatusState } from '$lib/ocr-status.svelte';
	import { debugRefundAvailable, requestDebugRefund } from '$lib/app-store-purchase';

	let loading = $state(true);
	let loadErrorMessage = $state('');

	/** 保存・エクスポートで起きた通信エラー。書き込み系の失敗は各ダイアログと揃えてモーダルで表示する。 */
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	/** CSV の取り込みを終えて戻ったときの知らせ。 */
	let importedDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	/** ブラウザで CSV を書き出したときの知らせ。 */
	let exportedDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	/** 写真の送信への同意を取り消したときの知らせ。 */
	let consentWithdrawnDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	function showError(title: string, message: string) {
		errorDialog?.show(title, message);
	}

	let timezone = $state('');
	let timezoneAuto = $state(false);
	let thresholds = $state<PeriodThresholds>({
		morningStartMin: 0,
		morningEndMin: 0,
		eveningStartMin: 0,
		eveningEndMin: 0
	});
	/** 朝・夜の重なりのエラーを出す行。 */
	let lastEditedPeriod = $state<DayPeriod>('morning');
	let periodErrors = $derived(periodThresholdErrors(thresholds, lastEditedPeriod));

	/** 手動の選択肢。サーバーが配る (保存の検証に通る名前だけが来る)。 */
	let timezones = $state<string[]>([]);
	let timezoneOptions = $derived(
		timezoneChoiceOptions(
			// 今の設定が一覧に無ければ足す (選択が外れて、別のゾーンを黙って保存してしまうのを防ぐ)。
			!timezoneAuto && timezone !== '' && !timezones.includes(timezone)
				? [...timezones, timezone]
				: timezones
		)
	);

	const periodRows = [
		{
			period: 'morning',
			label: m.record_day_period_morning_label(),
			labelClass: 'text-teal-foreground',
			startKey: 'morningStartMin',
			startLabel: m.settings_morning_start_label(),
			endKey: 'morningEndMin',
			endLabel: m.settings_morning_end_label()
		},
		{
			period: 'evening',
			label: m.record_day_period_evening_label(),
			labelClass: 'text-coral-foreground',
			startKey: 'eveningStartMin',
			startLabel: m.settings_evening_start_label(),
			endKey: 'eveningEndMin',
			endLabel: m.settings_evening_end_label()
		}
	] as const;

	/** 最後に保存できた設定。保存に失敗したら表示をこれに戻す。表示には使わないため `$state` にしない。 */
	let savedSettings: Settings | null = null;

	function apply(settings: Settings) {
		savedSettings = settings;
		timezone = settings.timezone;
		timezoneAuto = settings.timezoneAuto;
		thresholds = periodThresholdsOf(settings);
	}

	async function loadSettings() {
		loading = true;
		loadErrorMessage = '';
		// 選択肢と現在値が揃ってから描画する (先に片方だけ反映すると、保存済みの
		// タイムゾーンが選択肢に無い一瞬が生まれて選択が外れる)。
		const [settings, zones] = await Promise.all([fetchSettings(), fetchTimezones()]);
		loading = false;
		if (!settings.ok) {
			loadErrorMessage = settings.message;
			return;
		}
		if (!zones.ok) {
			loadErrorMessage = zones.message;
			return;
		}
		timezones = zones.data.timezones;
		apply(settings.data);
	}

	/** 保存中か・保存中に次の変更があったか。表示には使わないため `$state` にしない。 */
	let saving = false;
	let saveQueued = false;

	/** 保存できたことを支援技術に伝える文言。見た目の知らせ (`savedSections`) は読み上げない。 */
	let savedAnnouncement = $state('');

	type AutoSaveSection = 'period' | 'timezone';
	/** 変えたがまだ送っていない欄の段。送るときに取り出し、保存できたらその段にだけ「保存しました」を出す
	 * (送信中に別の段を変えたとき、まだ保存していない段に出さないため)。 */
	// 表示に使わないので、変化を追う SvelteSet にしない。
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	let unsentSections = new Set<AutoSaveSection>();
	/** 「保存しました」を出している段。すぐ保存して閉じる画面が無いので、段ごとに保存してから
	 * しばらく出して消す (別の段が後から保存されても、先の段の知らせを縮めない)。 */
	let savedSections = $state<AutoSaveSection[]>([]);
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- 表示に使わない
	const savedSectionTimers = new Map<AutoSaveSection, ReturnType<typeof setTimeout>>();
	const SAVED_NOTICE_MS = 3000;

	function showSaved(sections: Set<AutoSaveSection>) {
		for (const section of sections) {
			clearTimeout(savedSectionTimers.get(section));
			if (!savedSections.includes(section)) savedSections = [...savedSections, section];
			savedSectionTimers.set(
				section,
				setTimeout(() => {
					savedSections = savedSections.filter((shown) => shown !== section);
					savedSectionTimers.delete(section);
				}, SAVED_NOTICE_MS)
			);
		}
	}

	onDestroy(() => savedSectionTimers.forEach((timer) => clearTimeout(timer)));

	/** 変更をすぐ保存する。送信は1件ずつにし、保存中の変更は終わってから最新の値で送り直す
	 * (並べて送ると、応答の順序しだいで古い値が後から保存されうるため)。 */
	async function save() {
		if (saving) {
			saveQueued = true;
			return;
		}
		saving = true;
		try {
			do {
				saveQueued = false;
				// 規則に反する組み合わせは送らず、欄の下のエラーで直してもらう。
				if (periodErrors.morning !== undefined || periodErrors.evening !== undefined) return;
				const sending = unsentSections;
				unsentSections = new Set();
				const result = await saveSettings({ timezone, timezoneAuto, ...thresholds });
				if (!result.ok) {
					failSave(result.message);
					return;
				}
				if (saveQueued) {
					// 送信中に選び直された値を、応答の値で上書きしない。
					savedSettings = result.data;
				} else {
					apply(result.data);
				}
				// 他の画面の「今日」や日時の表示を新しいタイムゾーンで求め直すため、ログイン中ユーザーを読み直す。
				if (
					result.data.timezone !== page.data.user?.timezone ||
					result.data.timezoneAuto !== page.data.user?.timezoneAuto
				) {
					await invalidateAll();
				}
				// 同じ文言でも読み上げ直されるよう、一度空にしてから入れる。
				savedAnnouncement = '';
				await tick();
				savedAnnouncement = m.settings_saved_message();
				showSaved(sending);
			} while (saveQueued);
		} catch {
			// API の失敗は `saveSettings()` が結果で返すので、ここに来るのは読み直し (`invalidateAll`) 等の失敗。
			failSave(GENERIC_ERROR_MESSAGE());
		} finally {
			saving = false;
		}
	}

	/** 欄の値と保存された値が食い違ったままにならないよう、表示を保存済みの値に戻してから知らせる。 */
	function failSave(message: string) {
		// 送っていない変更も保存済みの値に戻すので、送る段は残さない。
		unsentSections.clear();
		if (savedSettings) apply(savedSettings);
		showError(m.settings_save_error_title(), message);
	}

	function setThreshold(period: DayPeriod, key: keyof PeriodThresholds, minutes: number) {
		thresholds[key] = minutes;
		lastEditedPeriod = period;
		unsentSections.add('period');
		save();
	}

	/** 「自動設定」の選択肢の値。IANA 名と重ならない。 */
	const AUTO_TIMEZONE = 'auto';

	/** 「自動設定」で保存されるゾーン。選択肢のラベルにも使い、選ぶ前に見えた名前と
	 * 選んだ後に保存される値が食い違わないようにする。 */
	const detectedTimezone = browserTimeZone();

	/** 「自動設定」を選んだら、ブラウザのタイムゾーンを自動として保存する (取れなければ今の値のまま)。 */
	function setTimezone(value: string) {
		if (value === AUTO_TIMEZONE) {
			timezoneAuto = true;
			timezone = detectedTimezone ?? timezone;
		} else {
			timezoneAuto = false;
			timezone = value;
		}
		unsentSections.add('timezone');
		save();
	}

	let logoutDialog = $state<ReturnType<typeof LogoutDialog> | null>(null);

	let deleteAccountDialog = $state<ReturnType<typeof DeleteAccountDialog> | null>(null);

	let unlinkDialog = $state<ReturnType<typeof UnlinkIdentityDialog> | null>(null);
	let pageHeading = $state<HTMLElement | null>(null);
	let loginMethodsHeading = $state<HTMLElement | null>(null);

	/** 連携を解除した後は、押したボタンが行ごと (最後の1件なら欄ごと) 消えてフォーカスが行き場を失うので、
	 * 欄の見出し (欄が消えたらページの見出し) へ移す。 */
	async function focusAfterUnlink() {
		await tick();
		(loginMethodsHeading ?? pageHeading)?.focus();
	}

	/** 連携している方法。ログイン画面と同じ順に並べる。 */
	let linkedProviders = $derived(
		LINKED_PROVIDERS.filter((p) => page.data.user?.linkedProviders.includes(p))
	);
	let canUnlink = $derived(page.data.user ? canUnlinkIdentity(page.data.user) : false);

	/** 問い合わせ先の URL。問い合わせが返るまで・取れなかったときは `null` で、行を出さない。 */
	let contactUrl = $state<string | null>(null);
	const aboutPages = [
		{ href: resolve('/help'), label: m.help_title() },
		{ href: resolve('/terms'), label: m.terms_title() },
		{ href: resolve('/privacy'), label: m.privacy_title() },
		{ href: resolve('/licenses'), label: m.licenses_title() }
	];

	/** エクスポート処理中かどうか (同時に1つしか実行しない想定)。 */
	let exporting = $state(false);
	let importFileInput = $state<HTMLInputElement | null>(null);

	/** 取り込むファイルをすぐ選ばせ、選んだら取り込みのページへ渡す (docs/import-export.md)。選んでいる間にも
	 * 取り込みのページのコードを読み込む (開いたときの読み込みが失敗していたとき)。 */
	function pickImportFile() {
		importFileHandoff.preload();
		openFilePicker(importFileInput);
	}

	function handleImportFileChange(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		importFileHandoff.set(file);
		goto(resolve('/settings/import'));
	}

	async function handleExport() {
		if (exporting) return;
		exporting = true;
		const result = await exportRecords();
		exporting = false;
		if (!result.ok) {
			showError(m.settings_export_error_title(), result.message);
		} else if (!isNativeApp) {
			// ブラウザは保存先を黙って決めるので、何という名前でどこに保存したかを知らせる。
			// アプリは共有シートで保存先を選ぶので要らない。
			exportedDialog?.show(
				m.settings_export_done_title(),
				m.settings_export_done_description({ filename: result.filename })
			);
		}
	}

	/** 遅れて返った古い応答 (開いたときの取得と、買い足した後の取り直し) で、新しい値を上書きしない。 */
	const ocrStatus = new OcrStatusState();
	/** 写真を Gemini API に送ることに同意しているか。同意しているときだけ、取り消す行を出す
	 * (docs/ocr.md)。 */
	let ocrConsented = $derived(ocrStatus.status.consented === true);
	let withdrawingOcrConsent = $state(false);
	/** 読み取りの枠を使う順に。無制限なら `null` で出さない (docs/ocr.md)。 */
	let ocrQuotas = $derived(ocrStatus.status.quotas);
	let ocrQuotaLow = $derived(ocrStatus.status.quotaLow);
	/** 読み取りを買い足せるか。残りによらず、いつでも買える (失効が無いので先に買っても損をしない)。 */
	let ocrTopupAvailable = $derived(ocrStatus.status.topupAvailable);
	let topupDialog = $state<ReturnType<typeof TopupConfirmDialog> | null>(null);
	/** 開発用の返金の申し出 (Debug ビルドのアプリだけ)。Sandbox で返金の通知を確かめるため。 */
	let debugRefund = $state(false);
	/** アプリの版。ウェブはいつ開いても最新なので出さない。 */
	let versionLabel = $state<string | null>(null);
	let requestingDebugRefund = $state(false);

	async function handleDebugRefund() {
		if (requestingDebugRefund) return;
		requestingDebugRefund = true;
		const message = await requestDebugRefund();
		requestingDebugRefund = false;
		if (message) showError(m.settings_debug_refund_error_title(), message);
	}

	async function withdrawConsent() {
		if (withdrawingOcrConsent) return;
		withdrawingOcrConsent = true;
		const result = await withdrawOcrConsent();
		withdrawingOcrConsent = false;
		if (!result.ok) {
			showError(m.settings_ocr_consent_withdraw_error_title(), result.message);
			return;
		}
		ocrStatus.setConsented(false);
		consentWithdrawnDialog?.show(
			m.settings_ocr_consent_withdrawn_title(),
			m.settings_ocr_consent_withdrawn_message()
		);
	}

	onMount(() => {
		importFileHandoff.preload();
		loadSettings();
		fetchContactUrl().then((url) => (contactUrl = url));
		if (isNativeApp) {
			debugRefundAvailable().then((available) => (debugRefund = available));
			appVersionLabel().then((label) => (versionLabel = label));
		}
		ocrStatus.refresh().then(async () => {
			// 写真で記録の「ほかの枠を見る」から来たら、節が描かれてから節へ移る。
			if (page.url.hash === '#ocr') {
				await tick();
				document.getElementById('ocr')?.scrollIntoView();
			}
		});
		// 取り込みのページは黙って戻るので、済んだことをここで知らせる。
		const imported = takeImportResult();
		if (imported) {
			importedDialog?.show(
				m.settings_import_done_title(),
				m.settings_import_done_description({ count: imported.created })
			);
		}
	});
</script>

{#snippet autoSaveTitle(section: AutoSaveSection, id: string, title: string)}
	<!-- 保存したことが見えるよう、見出しの右に「保存しました」を出す (デザインには無い)。
	     読み上げは role="status" の側で伝える。 -->
	<div class="mb-2 flex items-center justify-between gap-2">
		<h2 {id} class={cn(SECTION_TITLE_CLASS, 'mb-0')}>{title}</h2>
		{#if savedSections.includes(section)}
			<span class="flex items-center gap-1 text-sm text-muted-foreground" aria-hidden="true">
				<CheckIcon size={16} strokeWidth={2.2} />{m.settings_saved_badge()}
			</span>
		{/if}
	</div>
{/snippet}

<!-- デザインどおり、共通ヘッダーの代わりに、丸い戻るボタンと見出しを置く。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<PageHeader backHref={resolve('/')} title={m.settings_title()} bind:heading={pageHeading} />

	<div class="flex flex-col gap-4">
		{#if loading}
			<LoadingIndicator />
		{:else if loadErrorMessage}
			<LoadError message={loadErrorMessage} onretry={loadSettings} />
		{:else}
			<section aria-labelledby="period-heading">
				{@render autoSaveTitle('period', 'period-heading', m.settings_period_section_title())}
				<div class="flex flex-col gap-3.5 rounded-lg raised bg-card p-4">
					{#each periodRows as row, i (row.period)}
						{@const error = periodErrors[row.period]}
						{@const errorId = `${row.period}-period-error`}
						<!-- 欄の下の説明 (平均とグラフに使う時間帯) を、時刻の欄から読み上げる。 -->
						{@const describedby = error ? `${errorId} period-description` : 'period-description'}
						{#if i > 0}
							<Separator />
						{/if}
						<div class="flex flex-col gap-2">
							<div class="flex items-center justify-between gap-2">
								<span class={['text-sm font-bold', row.labelClass]}>{row.label}</span>
								<div class="flex items-center gap-2">
									<TimePicker
										label={row.startLabel}
										invalid={error !== undefined}
										{describedby}
										bind:value={
											() => thresholds[row.startKey],
											(minutes) => setThreshold(row.period, row.startKey, minutes)
										}
									/>
									<span class="text-subtle-foreground" aria-hidden="true"
										>{m.common_range_separator()}</span
									>
									<TimePicker
										label={row.endLabel}
										invalid={error !== undefined}
										{describedby}
										bind:value={
											() => thresholds[row.endKey],
											(minutes) => setThreshold(row.period, row.endKey, minutes)
										}
									/>
								</div>
							</div>
							<FieldErrorList id={errorId} messages={[error]} />
						</div>
					{/each}
					<p id="period-description" class="text-sm leading-relaxed text-muted-foreground">
						{m.settings_period_description()}
					</p>
				</div>
			</section>

			<section aria-labelledby="timezone-heading">
				{@render autoSaveTitle('timezone', 'timezone-heading', m.settings_timezone_section_title())}
				<div class="relative rounded-lg raised bg-card">
					<!-- ADR: shadcn-svelte の Select ではなくネイティブの select を使う: 選択肢が
					     100件以上あり、モバイルではOS標準のピッカーの方が探しやすいため。 -->
					<select
						class="h-14 w-full appearance-none rounded-lg bg-transparent pr-12 pl-4 text-base outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
						aria-labelledby="timezone-heading"
						bind:value={() => (timezoneAuto ? AUTO_TIMEZONE : timezone), setTimezone}
					>
						<option value={AUTO_TIMEZONE}>{autoTimezoneLabel(detectedTimezone ?? timezone)}</option>
						{#each timezoneOptions as option (option.zone)}
							<option value={option.zone}>{option.label}</option>
						{/each}
					</select>
					<ChevronDownIcon
						size={18}
						strokeWidth={2.2}
						class="pointer-events-none absolute top-1/2 right-4 -translate-y-1/2 text-subtle-foreground"
						aria-hidden="true"
					/>
				</div>
			</section>
		{/if}

		<!-- CSV出力・インポート。デザインに無いセクションだが、導線は設定画面に置く
		     (→ docs/import-export.md)。設定の取得可否に関わらず描く (期間閾値の取得に失敗していても
		     CSV操作はできてよい)。 -->
		<section aria-labelledby="data-heading">
			<h2 id="data-heading" class={SECTION_TITLE_CLASS}>{m.settings_data_section_title()}</h2>
			<div class="divide-y overflow-hidden rounded-lg raised bg-card">
				<!-- 説明は押せる範囲に含めるため行の中に置く。ボタン名がラベルだけになるよう、説明は
				     読み上げから外して aria-describedby で伝える。 -->
				<button
					type="button"
					class={LIST_ROW_CLASS}
					disabled={exporting}
					aria-busy={exporting}
					aria-describedby="export-csv-description"
					onclick={handleExport}
				>
					<span class="flex flex-col gap-1">
						{m.settings_export_csv_button()}
						<span
							id="export-csv-description"
							class="text-sm leading-relaxed text-muted-foreground"
							aria-hidden="true"
						>
							{m.settings_export_csv_description()}
						</span>
					</span>
					{#if exporting}
						<Spinner class="text-subtle-foreground" aria-hidden="true" />
					{/if}
				</button>
				<button
					type="button"
					class={LIST_ROW_CLASS}
					aria-describedby="import-description"
					onclick={pickImportFile}
				>
					<span class="flex flex-col gap-1">
						{m.settings_import_button()}
						<span
							id="import-description"
							class="text-sm leading-relaxed text-muted-foreground"
							aria-hidden="true"
						>
							{m.import_description()}
						</span>
					</span>
				</button>
			</div>
			<input
				bind:this={importFileInput}
				type="file"
				accept={CSV_FILE_ACCEPT}
				class="hidden"
				onchange={handleImportFileChange}
			/>
		</section>

		<!-- 読み取りの枠の一覧と買い足し (docs/ocr.md)、同意の取り消し
		     (App Store Review Guidelines 5.1.1(ii))。枠が無制限で同意もしていなければ、出すものが無いので節ごと出さない。 -->
		{#if ocrQuotas !== null || ocrConsented}
			<section id="ocr" aria-labelledby="ocr-heading">
				<h2 id="ocr-heading" class={SECTION_TITLE_CLASS}>{m.settings_ocr_section_title()}</h2>
				<div class="divide-y overflow-hidden rounded-lg raised bg-card">
					{#each ocrQuotas ?? [] as quota, index (index)}
						<div class="flex flex-col gap-2 px-4 py-3.5">
							<div class="flex items-baseline justify-between gap-2">
								<span class="text-base font-bold">
									{quota.kind === 'paid' ? m.ocr_quota_paid_title() : m.ocr_quota_free_title()}
								</span>
								<span class="text-xs text-subtle-foreground">
									{#if quota.purchasedAt}
										{m.ocr_quota_purchased_at({
											date: formatDateLabel(new Date(quota.purchasedAt), true, userTimeZone())
										})}
									{:else if index > 0}
										{m.ocr_quota_free_note()}
									{/if}
								</span>
							</div>
							<OcrQuotaGauge percent={quota.remainingPercent} low={ocrQuotaLow} />
						</div>
					{/each}
					{#if ocrTopupAvailable}
						<div class="px-4 py-3.5">
							<Button
								type="button"
								variant="outline"
								class="w-full"
								onclick={() => topupDialog?.show()}
							>
								{m.settings_ocr_topup_button()}
							</Button>
						</div>
					{/if}
					{#if debugRefund}
						<div class="px-4 py-3.5">
							<LoadingButton
								type="button"
								variant="outline"
								class="w-full"
								loading={requestingDebugRefund}
								onclick={handleDebugRefund}
							>
								{m.settings_debug_refund_button()}
							</LoadingButton>
						</div>
					{/if}
					{#if ocrConsented}
						<div class="flex min-h-13 items-center justify-between gap-2 px-4 py-2">
							<span class="py-1.5 text-base">{m.settings_ocr_consent_label()}</span>
							<LoadingButton
								type="button"
								variant="outline"
								loading={withdrawingOcrConsent}
								onclick={withdrawConsent}
							>
								{m.settings_ocr_consent_withdraw_button()}
							</LoadingButton>
						</div>
					{/if}
				</div>
			</section>
		{/if}

		<!-- 設定の取得可否に関わらず描く (設定APIだけが失敗した状態でもログアウトできる
		     必要がある)。 -->
		<section aria-labelledby="account-heading">
			<h2 id="account-heading" class={SECTION_TITLE_CLASS}>
				{m.settings_account_section_title()}
			</h2>
			<div class="divide-y overflow-hidden rounded-lg raised bg-card">
				<p class="px-4 py-3.5 text-base text-subtle-foreground">{page.data.user?.username}</p>
				<a href={resolve('/settings/change-email')} class={LIST_ROW_CLASS}>
					{m.settings_change_email_button()}
					<ChevronRightIcon
						size={16}
						strokeWidth={2.2}
						class="text-subtle-foreground"
						aria-hidden="true"
					/>
				</a>
				<!-- Google 専用アカウントは変更できるパスワードを持たないため、行自体を出さない。 -->
				{#if page.data.user?.passwordUsable}
					<a href={resolve('/settings/change-password')} class={LIST_ROW_CLASS}>
						{m.settings_change_password_button()}
						<ChevronRightIcon
							size={16}
							strokeWidth={2.2}
							class="text-subtle-foreground"
							aria-hidden="true"
						/>
					</a>
				{/if}
				<button
					type="button"
					class={cn(LIST_ROW_CLASS, 'justify-start text-coral-foreground')}
					onclick={() => logoutDialog?.show()}
				>
					<LogOutIcon size={16} strokeWidth={2.2} aria-hidden="true" />
					{m.common_logout_button()}
				</button>
			</div>
		</section>

		<!-- 連携している方法だけを出す。新しく連携する操作は持たない (ログインで自動で連携される、
		     docs/authentication.md)。最後のログイン方法は解除できないので、ボタンの代わりに理由を出す。 -->
		{#if linkedProviders.length > 0}
			<section aria-labelledby="login-methods-heading">
				<h2
					id="login-methods-heading"
					bind:this={loginMethodsHeading}
					tabindex="-1"
					class={cn(SECTION_TITLE_CLASS, 'outline-none')}
				>
					{m.settings_login_methods_section_title()}
				</h2>
				<div class="divide-y overflow-hidden rounded-lg raised bg-card">
					{#each linkedProviders as provider (provider)}
						<div class="flex min-h-13 items-center justify-between gap-2 px-4 py-2">
							<span class="flex flex-col gap-1 py-1.5 text-base">
								{linkedProviderLabel(provider)}
								{#if !canUnlink}
									<span class="text-sm leading-relaxed text-muted-foreground">
										{m.settings_unlink_last_method_note()}
									</span>
								{/if}
							</span>
							{#if canUnlink}
								<Button
									type="button"
									variant="outline"
									aria-label={`${linkedProviderLabel(provider)}: ${m.settings_unlink_button()}`}
									onclick={() => unlinkDialog?.show(provider)}
								>
									{m.settings_unlink_button()}
								</Button>
							{/if}
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<!-- 読むもの (使い方・規約) と問い合わせ先。ログインした後もたどれるようにする。
		     問い合わせ先は、URL が分かるまで出さない。 -->
		<section aria-labelledby="about-heading">
			<h2 id="about-heading" class={SECTION_TITLE_CLASS}>{m.settings_about_section_title()}</h2>
			<div class="divide-y overflow-hidden rounded-lg raised bg-card">
				{#each aboutPages as { href, label } (href)}
					<a {href} class={LIST_ROW_CLASS}>
						{label}
						<ChevronRightIcon
							size={16}
							strokeWidth={2.2}
							class="text-subtle-foreground"
							aria-hidden="true"
						/>
					</a>
				{/each}
				{#if contactUrl}
					<!-- 外のサイト (amiiby.com) のフォームなので、別のタブで開く。 -->
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
					<a href={contactUrl} target="_blank" rel="noopener" class={LIST_ROW_CLASS}>
						<span
							>{m.settings_contact_link()}<span class="sr-only">{m.common_opens_in_new_tab()}</span
							></span
						>
						<ExternalLinkIcon
							size={16}
							strokeWidth={2.2}
							class="text-subtle-foreground"
							aria-hidden="true"
						/>
					</a>
				{/if}
			</div>
		</section>

		<!-- ラベルの「...」通りモーダルで確認する (docs/authentication.md)。 -->
		<section aria-labelledby="danger-heading">
			<h2 id="danger-heading" class={cn(SECTION_TITLE_CLASS, 'text-destructive')}>
				{m.settings_danger_section_title()}
			</h2>
			<!-- 予約した後は予定日を出す。押し直しても予定日は変わらないため、ボタンは出さない (docs/authentication.md)。 -->
			{#if page.data.user?.deletionScheduledAt}
				<div class="flex flex-col gap-1 rounded-lg raised bg-card px-4 py-3.5 text-base">
					<p class="font-bold text-destructive">
						{m.settings_deletion_scheduled({
							date: formatFutureDateLabel(
								new Date(page.data.user.deletionScheduledAt),
								userTimeZone()
							)
						})}
					</p>
					{#if page.data.user.deletionOrigin === 'user'}
						<p class="text-sm leading-relaxed text-muted-foreground">
							{m.settings_deletion_cancel_hint()}
						</p>
					{/if}
				</div>
			{:else}
				<Button
					type="button"
					variant="destructive"
					class="h-14 w-full justify-start rounded-lg px-4 text-base font-bold"
					onclick={() => deleteAccountDialog?.show()}
				>
					{m.common_delete_account_button()}
				</Button>
			{/if}
		</section>

		<!-- 問い合わせで入っている版を聞けるよう、ページの一番下に出す (デザインには無い)。 -->
		{#if versionLabel}
			<p class="text-center text-sm text-subtle-foreground">
				{m.settings_app_version_label({ version: versionLabel })}
			</p>
		{/if}
	</div>
</div>

<p class="sr-only" role="status">{savedAnnouncement}</p>

<DeleteAccountDialog bind:this={deleteAccountDialog} />

<LogoutDialog bind:this={logoutDialog} />

<UnlinkIdentityDialog bind:this={unlinkDialog} onunlinked={focusAfterUnlink} />

<NoticeDialog bind:this={errorDialog} />
<NoticeDialog bind:this={importedDialog} />
<NoticeDialog bind:this={exportedDialog} />
<NoticeDialog bind:this={consentWithdrawnDialog} />
<TopupConfirmDialog bind:this={topupDialog} onpurchased={() => void ocrStatus.refresh()} />
