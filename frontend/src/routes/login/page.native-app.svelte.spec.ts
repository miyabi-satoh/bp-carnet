// アプリ (Capacitor) の中で開いたログイン画面 (docs/mobile-app.md)。
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/navigation', () => ({
	afterNavigate: vi.fn(),
	goto: vi.fn(),
	invalidateAll: vi.fn(),
	replaceState: vi.fn()
}));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$app/state', () => ({ page: { url: new URL('http://localhost/login'), data: {} } }));
vi.mock('$lib/native-app', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/native-app')>()),
	isNativeApp: true
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	fetchAuthProviders: vi.fn(),
	isSetupRequired: vi.fn(async () => false),
	loginWithApple: vi.fn(),
	loginWithGoogle: vi.fn(),
	loginWithLine: vi.fn()
}));
import { fetchAuthProviders, loginWithApple, loginWithGoogle, loginWithLine } from '$lib/auth';
import LoginPage from './+page.svelte';

const providers = {
	googleEnabled: true,
	lineEnabled: true,
	appleEnabled: false,
	appleAppEnabled: true,
	betaNotice: false,
	termsVersion: '1',
	contactUrl: 'https://example.com/contact',
	introUrl: ''
};

beforeEach(() => {
	vi.mocked(fetchAuthProviders).mockResolvedValue({ ok: true, data: providers });
});

/** SDK が結果を返さないまま待たせる。 */
const pending = () => new Promise<never>(() => {});

describe('各社の SDK でのログイン中のボタン', () => {
	it('Google は、アプリに戻ったらボタンを押せるように戻す (GoogleSignIn-iOS #578)', async () => {
		vi.mocked(loginWithGoogle).mockReturnValue(pending());
		const screen = await render(LoginPage);
		const google = screen.getByRole('button', { name: m.login_google_button() });
		await google.click();
		await expect.element(google).toBeDisabled();
		await expect.element(screen.getByText(m.login_signing_in())).toBeVisible();

		document.dispatchEvent(new Event('visibilitychange'));
		await expect.element(google).toBeEnabled();
		await expect.element(screen.getByText(m.login_signing_in())).not.toBeInTheDocument();
		await expect.element(screen.getByRole('button', { name: m.login_line_button() })).toBeEnabled();
	});

	it('押し直したあとに前の Google の結果が届いても、押し直した分を待ち続ける', async () => {
		let finishFirst: (result: { ok: false; message: string }) => void = () => {};
		vi.mocked(loginWithGoogle)
			.mockReturnValueOnce(new Promise((resolve) => (finishFirst = resolve)))
			.mockReturnValueOnce(pending());
		const screen = await render(LoginPage);
		const google = screen.getByRole('button', { name: m.login_google_button() });
		await google.click();
		document.dispatchEvent(new Event('visibilitychange'));
		await google.click();
		await expect.element(google).toBeDisabled();

		finishFirst({ ok: false, message: 'late' });
		await new Promise((r) => setTimeout(r, 50));
		await expect.element(google).toBeDisabled();
		await expect.element(screen.getByText('late')).not.toBeInTheDocument();
	});

	it('LINE は、アプリに戻っても押せないまま待つ', async () => {
		vi.mocked(loginWithLine).mockReturnValue(pending());
		const screen = await render(LoginPage);
		const line = screen.getByRole('button', { name: m.login_line_button() });
		await line.click();
		await expect.element(line).toBeDisabled();

		document.dispatchEvent(new Event('visibilitychange'));
		await new Promise((r) => setTimeout(r, 50));
		await expect.element(line).toBeDisabled();
	});
});

describe('Apple のボタン', () => {
	it('アプリでも出し、押すと Sign in with Apple でログインする', async () => {
		vi.mocked(loginWithApple).mockReturnValue(pending());
		const screen = await render(LoginPage);
		const apple = screen.getByRole('button', { name: m.login_apple_button() });
		await apple.click();
		expect(loginWithApple).toHaveBeenCalled();
		await expect.element(apple).toBeDisabled();
	});

	it('アプリで受け付けない構成では、ウェブで使えても出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, appleEnabled: true, appleAppEnabled: false }
		});
		const screen = await render(LoginPage);
		await expect
			.element(screen.getByRole('button', { name: m.login_google_button() }))
			.toBeVisible();
		await expect
			.element(screen.getByRole('button', { name: m.login_apple_button() }))
			.not.toBeInTheDocument();
	});
});

describe('紹介ページへのリンク (docs/mobile-app.md)', () => {
	it('introUrl があっても、アプリには出さない', async () => {
		vi.mocked(fetchAuthProviders).mockResolvedValue({
			ok: true,
			data: { ...providers, introUrl: 'https://example.com/app/' }
		});
		const screen = await render(LoginPage);
		await expect.element(screen.getByRole('link', { name: m.login_help_link() })).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.login_intro_link() }))
			.not.toBeInTheDocument();
	});
});
