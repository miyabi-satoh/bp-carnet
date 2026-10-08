import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { APP_STORE_URL } from '$lib/app-store-link';

vi.mock('$lib/ios-web-app', () => ({ isIosWebApp: vi.fn(() => true) }));
import { isIosWebApp } from '$lib/ios-web-app';
import AppStoreNotice from './app-store-notice.svelte';

beforeEach(() => {
	localStorage.clear();
	vi.mocked(isIosWebApp).mockReturnValue(true);
});

describe('AppStoreNotice', () => {
	it('ホーム画面のウェブアプリでは、App Store のページへの案内を出す', async () => {
		const screen = await render(AppStoreNotice);

		await expect
			.element(screen.getByRole('heading', { name: m.app_store_notice_title() }))
			.toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.app_store_notice_link() }))
			.toHaveAttribute('href', APP_STORE_URL);
	});

	it('ブラウザで開いたときは出さない (Safari の Smart App Banner に任せる)', async () => {
		vi.mocked(isIosWebApp).mockReturnValue(false);
		const screen = await render(AppStoreNotice);

		await expect
			.element(screen.getByRole('heading', { name: m.app_store_notice_title() }))
			.not.toBeInTheDocument();
	});

	it('閉じたら、開き直しても出さない', async () => {
		const first = await render(AppStoreNotice);
		await first.getByRole('button', { name: m.common_close_button() }).click();
		await expect
			.element(first.getByRole('heading', { name: m.app_store_notice_title() }))
			.not.toBeInTheDocument();
		first.unmount();

		const second = await render(AppStoreNotice);
		await expect
			.element(second.getByRole('heading', { name: m.app_store_notice_title() }))
			.not.toBeInTheDocument();
	});
});
