// ログインの方法の問い合わせ (`/auth/providers`) の応答を差し替える。e2e の backend は LINE・Google・Apple を
// 使わない構成なので、使う構成の画面はこれで確かめる。
import type { BrowserContext, Page } from '@playwright/test';
import type { components } from '../src/lib/api/schema';

type AuthProvidersResponse = components['schemas']['AuthProvidersResponse'];

/** 本番の構成 (deploy/cloud/config.production.toml) で有効な、LINE・Google のログイン。Apple・オープンβの表示は、使う側で足す。 */
export const PRODUCTION_AUTH_PROVIDERS = {
	googleEnabled: true,
	lineEnabled: true
} satisfies Partial<AuthProvidersResponse>;

/**
 * `overrides` に無い項目は、どの方法も使わない構成 (β表示も出さない) で答える。ページを開く前に呼ぶ。
 * 別のタブにも効かせたいときは、`page` ではなくコンテキストを渡す。
 */
export async function mockAuthProviders(
	target: Page | BrowserContext,
	overrides: Partial<AuthProvidersResponse>
): Promise<void> {
	const body: AuthProvidersResponse = {
		googleEnabled: false,
		lineEnabled: false,
		appleEnabled: false,
		appleAppEnabled: false,
		termsVersion: '1',
		betaNotice: false,
		contactUrl: 'https://example.com/contact/',
		introUrl: '',
		...overrides
	};
	await target.route('**/api/v1/auth/providers', (route) => route.fulfill({ json: body }));
}

type MeResponse = components['schemas']['MeResponse'];

/**
 * ログイン中ユーザーの応答 (`/auth/me`) の連携とパスワードの有無を差し替え、連携の解除
 * (`DELETE /account/identities/{provider}`) に答える。e2e の backend は LINE・Google でログインできない
 * ため、連携のある状態はこれで作る。ページを開く前に呼ぶ。
 * 解除は、ログイン方法が残らなければサーバーと同じく 409 で断り、残れば連携を外す (本当の判定は
 * backend の結合テスト、tests/api.rs)。`unlinkRequests` に、受けた解除の提供元が並ぶ。
 */
export async function mockLinkedProviders(
	page: Page,
	initial: { passwordUsable: boolean; linkedProviders: MeResponse['linkedProviders'] }
): Promise<{ unlinkRequests: string[] }> {
	const { passwordUsable } = initial;
	let { linkedProviders } = initial;
	const unlinkRequests: string[] = [];
	await page.route('**/api/v1/auth/me', async (route) => {
		const response = await route.fetch();
		const me: MeResponse = await response.json();
		await route.fulfill({ response, json: { ...me, passwordUsable, linkedProviders } });
	});
	await page.route('**/api/v1/account/identities/*', async (route) => {
		const provider = route
			.request()
			.url()
			.split('/')
			.pop() as MeResponse['linkedProviders'][number];
		unlinkRequests.push(provider);
		if (!passwordUsable && linkedProviders.length < 2) {
			await route.fulfill({
				status: 409,
				json: {
					error: {
						code: 'cannot_unlink_last_login_method',
						message: 'cannot unlink the last login method'
					}
				}
			});
			return;
		}
		linkedProviders = linkedProviders.filter((p) => p !== provider);
		await route.fulfill({ status: 204 });
	});
	return { unlinkRequests };
}
