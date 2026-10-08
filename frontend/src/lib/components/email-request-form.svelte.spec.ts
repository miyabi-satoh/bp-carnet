// メールのリンクを申し込む画面 (サインアップ・再設定の申し込み) の、送った後の表示。
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';
import EmailRequestForm from './email-request-form.svelte';

it('送った後は送り先を見せ、「メールアドレスを直す」で入れた値を残したまま入力に戻る', async () => {
	const sent: string[] = [];
	const screen = await render(EmailRequestForm, {
		submitLabel: '送る',
		sentTitle: '送りました',
		sentDescription: 'リンクを開いてください。',
		onsubmit: async (email: string) => {
			sent.push(email);
			return true;
		}
	});
	const emailInput = screen.getByLabelText(requiredLabel(m.common_email_label()), { exact: true });

	await emailInput.fill('kyoko@exmaple.com');
	await screen.getByRole('button', { name: '送る' }).click();
	await expect.element(screen.getByText('kyoko@exmaple.com')).toBeVisible();

	await screen.getByRole('button', { name: m.email_request_edit_button() }).click();
	await expect.element(emailInput).toHaveValue('kyoko@exmaple.com');
	await expect.element(emailInput).toHaveFocus();
	await emailInput.fill('kyoko@example.com');
	await screen.getByRole('button', { name: '送る' }).click();

	await expect.element(screen.getByText('kyoko@example.com')).toBeVisible();
	expect(sent).toEqual(['kyoko@exmaple.com', 'kyoko@example.com']);
});
