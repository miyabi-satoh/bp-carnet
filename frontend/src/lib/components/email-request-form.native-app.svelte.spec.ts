// アプリ (Capacitor) の中で、メールのリンクを申し込んだあとの画面 (docs/mobile-app.md)。
import { expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$lib/native-app', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/native-app')>()),
	isNativeApp: true
}));
import EmailRequestForm from './email-request-form.svelte';

it('送れたら、リンクはブラウザで開くので、済んだらアプリに戻るよう添える', async () => {
	const screen = await render(EmailRequestForm, {
		submitLabel: '送る',
		sentTitle: '送りました',
		sentDescription: 'リンクを開いてください。',
		onsubmit: async () => true
	});

	await screen
		.getByLabelText(requiredLabel(m.common_email_label()), { exact: true })
		.fill('kyoko@example.com');
	await screen.getByRole('button', { name: '送る' }).click();

	await expect.element(screen.getByText('送りました')).toBeVisible();
	await expect.element(screen.getByText(m.email_request_sent_app_note())).toBeVisible();
});
