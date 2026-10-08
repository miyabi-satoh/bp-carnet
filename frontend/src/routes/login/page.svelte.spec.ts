import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/navigation', () => ({
	afterNavigate: vi.fn(),
	goto: vi.fn(),
	invalidateAll: vi.fn(),
	replaceState: vi.fn()
}));
vi.mock('$app/paths', () => ({
	resolve: (path: string, params: Record<string, string> = {}) =>
		path.replace(/\[(\w+)\]/g, (_, key: string) => params[key])
}));
vi.mock('$app/state', () => ({ page: { url: new URL('http://localhost/login'), data: {} } }));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	fetchAuthProviders: vi.fn(),
	login: vi.fn()
}));
vi.mock('$lib/in-app-browser', () => ({ isInAppBrowser: vi.fn(() => false) }));
vi.mock('$lib/ios-web-app', () => ({ isIosWebApp: vi.fn(() => false) }));
import { goto } from '$app/navigation';
import { isInAppBrowser } from '$lib/in-app-browser';
import { isIosWebApp } from '$lib/ios-web-app';
import { fetchAuthProviders, login, type LoginResult } from '$lib/auth';
import LoginPage from './+page.svelte';

const providers = {
	googleEnabled: false,
	lineEnabled: false,
	appleEnabled: false,
	appleAppEnabled: false,
	termsVersion: '2',
	betaNotice: false,
	contactUrl: 'https://example.com/contact/',
	introUrl: ''
};

beforeEach(() => {
	vi.mocked(login).mockReset();
	vi.mocked(login).mockResolvedValue({ ok: true, deletionCancelled: false });
	vi.mocked(goto).mockReset();
	vi.mocked(isIosWebApp).mockReturnValue(false);
	vi.mocked(isInAppBrowser).mockReturnValue(false);
});

describe('通信できないとき (docs/mobile-app.md)', () => {
	it('ログイン方式を問い合わせられなければ、ボタンを消さずに「もう一度試す」を出し、押すと出し直す', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValueOnce({ ok: false, failure: 'offline' });
		vi.mocked(fetchAuthProviders).mockResolvedValueOnce({
			ok: true,
			data: { ...providers, googleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect.element(screen.getByText(m.layout_offline_title())).toBeVisible();
		await screen.getByRole('button', { name: m.common_retry_button() }).click();

		await expect.element(screen.getByRole('link', { name: m.login_google_button() })).toBeVisible();
	});
});

describe('ユーザーIDの入力欄', () => {
	// ユーザーIDはメールアドレスそのものなので、欄を「メールアドレス」と書く (docs/authentication.md)。
	it('入力したユーザーIDでログインする', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		const screen = await render(LoginPage);

		const username = screen.getByLabelText(m.common_email_label(), { exact: true });
		// スマートフォンで @ のあるキーボードを出し、先頭を大文字にしない。
		await expect.element(username).toHaveAttribute('inputmode', 'email');
		await expect.element(username).toHaveAttribute('autocapitalize', 'off');
		await username.fill('sakura');
		await screen.getByLabelText(m.common_password_label(), { exact: true }).fill('1234');
		await screen.getByRole('button', { name: m.login_submit_button() }).click();

		await vi.waitFor(() => expect(login).toHaveBeenCalledWith('sakura', '1234'));
		// ログイン画面は履歴に残さない。
		await vi.waitFor(() => expect(goto).toHaveBeenCalledWith('/', { replaceState: true }));
	});
});

describe('ログイン中の覆い', () => {
	it('結果を待つ間は「ログインしています...」で画面を覆い、失敗したら閉じてお知らせを出す', async () => {
		let finish: (result: LoginResult) => void = () => {};
		vi.mocked(login).mockReturnValue(new Promise((resolve) => (finish = resolve)));
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		const screen = await render(LoginPage);
		await screen.getByLabelText(m.common_email_label(), { exact: true }).fill('sakura');
		await screen.getByLabelText(m.common_password_label(), { exact: true }).fill('1234');
		await screen.getByRole('button', { name: m.login_submit_button() }).click();

		await expect.element(screen.getByText(m.login_signing_in())).toBeVisible();

		finish({ ok: false, message: 'failed' });
		await expect.element(screen.getByText('failed')).toBeVisible();
		await expect.element(screen.getByText(m.login_signing_in())).not.toBeInTheDocument();
	});
});

describe('オープンβテスト中の表示 (docs/architecture.md)', () => {
	it('betaNotice が true なら、バッジを出す。一文は出さない (画面に収めるため、印はバッジだけにする)', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, betaNotice: true }
		});
		const screen = await render(LoginPage);

		await expect.element(screen.getByRole('img', { name: m.beta_badge_label() })).toBeVisible();
		await expect.element(screen.getByText(m.beta_notice_message())).not.toBeInTheDocument();
	});

	it('betaNotice が false なら、バッジを出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByRole('button', { name: m.login_submit_button() }))
			.toBeVisible();
		await expect
			.element(screen.getByRole('img', { name: m.beta_badge_label() }))
			.not.toBeInTheDocument();
	});
});

describe('紹介ページへのリンク (docs/architecture.md)', () => {
	it('introUrl があれば、カードの下に紹介ページへのリンクを出す', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, introUrl: 'https://example.com/app/' }
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByRole('link', { name: m.login_intro_link() }))
			.toHaveAttribute('href', 'https://example.com/app/');
	});

	it('introUrl が空なら、リンクを出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		const screen = await render(LoginPage);

		await expect.element(screen.getByRole('link', { name: m.login_help_link() })).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.login_intro_link() }))
			.not.toBeInTheDocument();
	});
});

/** 同意の文の全文。リンクは別のタブで開くので、読み上げ用の知らせ (画面には出ない) も続く。 */
function consentText() {
	const newTab = m.common_opens_in_new_tab();
	return m.terms_consent_providers({
		terms: m.terms_title() + newTab,
		privacy: m.privacy_title() + newTab
	});
}

describe('LINE・Google・Apple のボタン (docs/authentication.md)', () => {
	for (const [lineEnabled, googleEnabled, appleEnabled, services] of [
		[true, true, true, 'LINE・Google・Apple'],
		[true, true, false, 'LINE・Google'],
		[true, false, false, 'LINE'],
		[false, true, false, 'Google'],
		[false, false, true, 'Apple']
	] as const) {
		it(`「${services}」が有効なら、そのボタンと、直上に同意の文を出す`, async () => {
			vi.mocked(fetchAuthProviders).mockResolvedValue({
				ok: true,
				data: { ...providers, lineEnabled, googleEnabled, appleEnabled }
			});
			const screen = await render(LoginPage);

			await expect.element(screen.getByText(consentText())).toBeVisible();
			for (const [enabled, name, provider] of [
				[lineEnabled, m.login_line_button(), 'line'],
				[googleEnabled, m.login_google_button(), 'google'],
				[appleEnabled, m.login_apple_button(), 'apple']
			] as const) {
				const link = screen.getByRole('link', { name });
				if (enabled) {
					await expect.element(link).toHaveAttribute('href', `/api/v1/auth/${provider}/login`);
				} else {
					await expect.element(link).not.toBeInTheDocument();
				}
			}
		});
	}
});

describe('iOS のホーム画面から開いたアプリ (docs/authentication.md)', () => {
	it('LINE のボタンを出さず、案内を出す。Google・Apple のボタンと同意の文は LINE を除いて残す', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(true);
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: {
				...providers,
				lineEnabled: true,
				googleEnabled: true,
				appleEnabled: true
			}
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice({ services: 'Google・Apple' })))
			.toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.login_line_button() }))
			.not.toBeInTheDocument();
		await expect.element(screen.getByRole('link', { name: m.login_google_button() })).toBeVisible();
		await expect.element(screen.getByRole('link', { name: m.login_apple_button() })).toBeVisible();
		await expect.element(screen.getByText(consentText())).toBeVisible();
		await expect.element(screen.getByText(m.login_separator())).toBeVisible();
	});

	it('案内で勧めるのは、出ているボタンだけ', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(true);
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true, appleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice({ services: 'Apple' })))
			.toBeVisible();
	});

	it('LINE だけの構成では、案内だけを出し、同意の文も「または」も出さない', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(true);
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true }
		});
		const screen = await render(LoginPage);

		// 代わりのボタンが無いので、ブラウザで開くことだけを勧める。
		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice_browser_only()))
			.toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.login_line_button() }))
			.not.toBeInTheDocument();
		await expect.element(screen.getByText(m.login_separator())).not.toBeInTheDocument();
		await expect.element(screen.getByText(consentText())).not.toBeInTheDocument();
	});

	it('LINE が無効なら、Google だけの構成でも案内を出さない', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(true);
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, googleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect.element(screen.getByRole('link', { name: m.login_google_button() })).toBeVisible();
		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice({ services: 'Google' })))
			.not.toBeInTheDocument();
	});

	it('iOS のホーム画面から開いていなければ、従来どおり LINE のボタンを出し、案内は出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true, googleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect.element(screen.getByRole('link', { name: m.login_line_button() })).toBeVisible();
		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice({ services: 'Google' })))
			.not.toBeInTheDocument();
	});
});

describe('内蔵ブラウザで開いたとき (docs/authentication.md)', () => {
	beforeEach(() => {
		vi.mocked(isInAppBrowser).mockReturnValue(true);
	});

	it('LINE・Google・Apple のボタンを出さず、ブラウザで開き直す案内とリンクのコピーを出す', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: {
				...providers,
				lineEnabled: true,
				googleEnabled: true,
				appleEnabled: true
			}
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByText(m.login_in_app_browser_notice({ services: 'LINE・Google・Apple' })))
			.toBeVisible();
		await expect
			.element(screen.getByRole('button', { name: m.login_in_app_browser_copy_button() }))
			.toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.login_line_button() }))
			.not.toBeInTheDocument();
		await expect
			.element(screen.getByRole('link', { name: m.login_google_button() }))
			.not.toBeInTheDocument();
		await expect
			.element(screen.getByRole('link', { name: m.login_apple_button() }))
			.not.toBeInTheDocument();
		await expect.element(screen.getByText(m.login_separator())).not.toBeInTheDocument();
		// ID/PW のログインは使える。
		await expect
			.element(screen.getByRole('button', { name: m.login_submit_button() }))
			.toBeVisible();
	});

	it('iOS のホーム画面のアプリと重なっても、案内は内蔵ブラウザのものだけを出す', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(true);
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true, googleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByText(m.login_in_app_browser_notice({ services: 'LINE・Google' })))
			.toBeVisible();
		await expect
			.element(screen.getByText(m.login_line_ios_web_app_notice({ services: 'Google' })))
			.not.toBeInTheDocument();
	});

	it('有効な方式の名前だけを案内に書く', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, googleEnabled: true }
		});
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByText(m.login_in_app_browser_notice({ services: 'Google' })))
			.toBeVisible();
	});

	it('LINE も Google も Apple も無い構成では、案内を出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		const screen = await render(LoginPage);

		await expect
			.element(screen.getByRole('button', { name: m.login_in_app_browser_copy_button() }))
			.not.toBeInTheDocument();
	});

	it('コピーのボタンを押すと、ログイン画面のアドレスをクリップボードへ入れて知らせる', async () => {
		const writeText = vi.fn().mockResolvedValue(undefined);
		vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true }
		});
		const screen = await render(LoginPage);

		await screen.getByRole('button', { name: m.login_in_app_browser_copy_button() }).click();

		expect(writeText).toHaveBeenCalledWith('http://localhost/login');
		await expect.element(screen.getByText(m.login_in_app_browser_copied())).toBeVisible();
		vi.unstubAllGlobals();
	});

	it('コピーに失敗したら、メニューから開くよう知らせる', async () => {
		const writeText = vi.fn().mockRejectedValue(new Error('denied'));
		vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, lineEnabled: true }
		});
		const screen = await render(LoginPage);

		await screen.getByRole('button', { name: m.login_in_app_browser_copy_button() }).click();

		await expect.element(screen.getByText(m.login_in_app_browser_copy_failed())).toBeVisible();
		vi.unstubAllGlobals();
	});
});

describe('ログインの失敗の問い合わせ先・使い方 (docs/authentication.md)', () => {
	const frozen = {
		ok: false as const,
		message: `${m.error_account_frozen()}${m.common_contact_operator()}`,
		code: 'account_frozen' as const
	};

	async function submitFrozen(result: LoginResult = frozen) {
		vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
		vi.mocked(login).mockResolvedValue(result);
		const screen = await render(LoginPage);
		await screen.getByLabelText(m.common_email_label(), { exact: true }).fill('sakura');
		await screen.getByLabelText(m.common_password_label(), { exact: true }).fill('1234');
		await screen.getByRole('button', { name: m.login_submit_button() }).click();
		return screen;
	}

	it('運営者への問い合わせとフォームへのリンクを出し、使い方へはリンクしない', async () => {
		const screen = await submitFrozen();
		const dialog = screen.getByRole('alertdialog');

		await expect
			.element(dialog.getByText(`${m.error_account_frozen()}${m.common_contact_operator()}`))
			.toBeVisible();
		await expect
			.element(dialog.getByRole('link', { name: new RegExp(m.common_contact_form_link()) }))
			.toHaveAttribute('href', 'https://example.com/contact/');
		await expect
			.element(dialog.getByRole('link', { name: m.notice_help_link() }))
			.not.toBeInTheDocument();
	});

	it('アプリの各社のログインでの衝突も、問い合わせ先を添える', async () => {
		const screen = await submitFrozen({
			ok: false,
			message: `${m.error_external_account_conflict()}${m.common_contact_operator()}`,
			code: 'external_account_conflict'
		});
		const dialog = screen.getByRole('alertdialog');

		await expect
			.element(
				dialog.getByText(`${m.error_external_account_conflict()}${m.common_contact_operator()}`)
			)
			.toBeVisible();
		await expect
			.element(dialog.getByRole('link', { name: new RegExp(m.common_contact_form_link()) }))
			.toBeVisible();
	});

	it('アプリの LINE でメールアドレスを断られたら、専用の使い方の項目へリンクする', async () => {
		const screen = await submitFrozen({
			ok: false,
			message: m.error_line_email_required(),
			code: 'line_email_required'
		});
		const dialog = screen.getByRole('alertdialog');

		await expect
			.element(dialog.getByRole('link', { name: m.notice_help_link() }))
			.toHaveAttribute('href', '/help/trouble-line-email');
	});
});
