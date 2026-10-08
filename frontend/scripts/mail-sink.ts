// backend が送るメールを受け取る SMTP サーバー (e2e・`just spec` の使い捨ての backend と、`just dev-mail`)。
// `node scripts/mail-sink.ts <dir> <port>` で単体でも立てられる (`just dev-mail`。Ctrl-C で止める)。
// 受け取ったメールは 1通ずつ JSON (`ReceivedMail`、→ e2e/mail-helpers.ts) にして `dir` に書き出す。
// e2e の Playwright のテストは別のプロセスで動くため、ファイルで受け渡す。
//
// backend 側は `[mail] tls = "none"` で送る。本物の SMTP で送る経路 (lettre) ごと確かめるため、
// 送る代わりに記録する `Mailer::recording` は使わない。

import { mkdirSync, renameSync, writeFileSync } from 'node:fs';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import mailparser from 'mailparser';
import smtpServer from 'smtp-server';

export interface MailSink {
	port: number;
	stop: () => Promise<void>;
}

/** `port` を省くと、空いているポートで待ち受ける。 */
export async function startMailSink(dir: string, port = 0): Promise<MailSink> {
	// 立て直しても前に受けたメールを上書きしないよう、名前の頭に立てた時刻を付ける (名前の順が届いた順になる)。
	const startedAt = Date.now();
	let count = 0;
	const server = new smtpServer.SMTPServer({
		authOptional: true,
		disabledCommands: ['STARTTLS'],
		logger: false,
		onData(stream, session, callback) {
			mailparser
				.simpleParser(stream)
				.then((mail) => {
					count += 1;
					const received = {
						to: session.envelope.rcptTo.map(({ address }) => address),
						subject: mail.subject ?? '',
						text: mail.text ?? ''
					};
					// 書きかけのファイルを読ませないよう、別名で書いてから名前を替える。
					const file = path.join(dir, `${startedAt}-${String(count).padStart(4, '0')}.json`);
					writeFileSync(`${file}.tmp`, JSON.stringify(received));
					renameSync(`${file}.tmp`, file);
					callback();
				})
				// 書き出しの失敗も SMTP の応答で返す。投げたままにすると親のプロセスごと落ちる。
				.catch((err: Error) => callback(err));
		}
	});
	await new Promise<void>((resolve, reject) => {
		server.once('error', reject);
		server.listen(port, '127.0.0.1', () => resolve());
	});
	const address = server.server.address() as AddressInfo;
	return {
		port: address.port,
		stop: () => new Promise((resolve) => server.close(() => resolve()))
	};
}

// `import.meta.main` は Node 24.2 からなので、起動したファイルと比べる。
if (
	process.argv[1] !== undefined &&
	path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
	const [dir, port] = process.argv.slice(2);
	if (dir === undefined || port === undefined) {
		console.error('使い方: node scripts/mail-sink.ts <dir> <port>');
		process.exit(1);
	}
	mkdirSync(dir, { recursive: true });
	const sink = await startMailSink(dir, Number(port));
	console.log(`メールを 127.0.0.1:${sink.port} で受け取り、${path.resolve(dir)} に置きます`);
	// `stop()` は開いたままの SMTP の接続 (動いている backend) が閉じるのを待つので、待たずに終える。
	for (const signal of ['SIGINT', 'SIGTERM'] as const) process.once(signal, () => process.exit(0));
}
