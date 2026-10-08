// backend が送ったメールを読む。`just e2e-local` は使い捨ての backend と一緒に立てる SMTP の受け口で受け取り、
// 1通ずつ JSON にして `E2E_MAIL_DIR` に置く (→ scripts/backend-process.ts・scripts/mail-sink.ts)。
// 起動済みの環境に当てる `just e2e` では届いたメールを読めないので、メールを読むテストは飛ばす。
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { expect } from '@playwright/test';

export const MAIL_DIR = process.env.E2E_MAIL_DIR;

export const MAIL_UNAVAILABLE_REASON =
	'届いたメールを読めるのは just e2e-local だけ (E2E_MAIL_DIR が無い)';

export type ReceivedMail = { to: string[]; subject: string; text: string };

function receivedMails(): ReceivedMail[] {
	if (MAIL_DIR === undefined) throw new Error(MAIL_UNAVAILABLE_REASON);
	return readdirSync(MAIL_DIR)
		.filter((name) => name.endsWith('.json'))
		.sort()
		.map((name) => JSON.parse(readFileSync(path.join(MAIL_DIR, name), 'utf8')) as ReceivedMail);
}

/** `to` 宛てのメールを受け取った順に返す。 */
function mailsTo(to: string): ReceivedMail[] {
	return receivedMails().filter((mail) => mail.to.includes(to));
}

/** `to` 宛てのメールが届くのを待ち、最後に届いたものを返す。backend は応答を返してから送る。 */
export async function waitForMailTo(to: string): Promise<ReceivedMail> {
	await expect.poll(() => mailsTo(to).length, { message: `${to} 宛てのメール` }).toBeGreaterThan(0);
	return mailsTo(to).at(-1)!;
}

/** `to` 宛ての、件名が `subject` のメールが届くのを待って返す。 */
export async function waitForMailWithSubject(to: string, subject: string): Promise<ReceivedMail> {
	const find = () => mailsTo(to).find((mail) => mail.subject === subject);
	await expect
		.poll(() => find() !== undefined, { message: `${to} 宛ての「${subject}」` })
		.toBe(true);
	return find()!;
}

/** 本文に載っている、`pathname` を開くリンク。 */
export function linkIn(mail: ReceivedMail, pathname: string): string {
	const link = mail.text
		.match(/https?:\/\/\S+/g)
		?.find((url) => new URL(url).pathname === pathname);
	if (link === undefined)
		throw new Error(`${pathname} へのリンクがメールにありません: ${mail.text}`);
	return link;
}
