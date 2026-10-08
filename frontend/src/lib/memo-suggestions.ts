// メモの候補。過去のメモを「、」で区切った言葉を、直近の期間でよく使う順に並べ、
// メモ欄の下にチップで出す。タップすると「、」を挟んで後ろに足す。

import { client } from '$lib/api/client';
import { unwrapRequest } from '$lib/api/errors';
import { formatDateOnly, localToday } from '$lib/period';

/** メモの言葉の区切り。候補を足すときにも挟む。 */
export const MEMO_SEPARATOR = '、';

/** 候補を数える期間 (日)。使わなくなった言葉が残り続けないように、直近だけを数える。 */
export const MEMO_SUGGESTION_DAYS = 90;

/** 一度に出す候補の数。 */
export const MEMO_SUGGESTION_LIMIT = 5;

/** メモを「、」で区切った言葉。空の言葉は除く。 */
export function memoPhrases(memo: string): string[] {
	return memo
		.split(MEMO_SEPARATOR)
		.map((phrase) => phrase.trim())
		.filter((phrase) => phrase !== '');
}

/** 過去の記録のメモから、言葉をよく使う順に並べる。同じ回数なら最近使ったものを先にする。 */
export function rankMemoPhrases(records: { memo: string; localMeasuredAt: string }[]): string[] {
	const stats = new Map<string, { count: number; last: string }>();
	for (const { memo, localMeasuredAt } of records) {
		for (const phrase of new Set(memoPhrases(memo))) {
			const stat = stats.get(phrase) ?? { count: 0, last: '' };
			stat.count += 1;
			if (localMeasuredAt > stat.last) stat.last = localMeasuredAt;
			stats.set(phrase, stat);
		}
	}
	return (
		[...stats.entries()]
			// `last` は ISO 形式の日時なので、文字列のまま比べれば時刻の順になる。
			.sort(([, a], [, b]) => b.count - a.count || (b.last > a.last ? 1 : b.last < a.last ? -1 : 0))
			.map(([phrase]) => phrase)
	);
}

/**
 * メモ欄の最後の言葉が打ちかけなら、その言葉。無ければ `null`。
 * 候補のどれとも一致しないか、まだメモに入っていないほかの候補に含まれる (「頭痛」に対する「頭痛薬」) なら打ちかけとみなす。
 */
function typingPhrase(memo: string, ranked: readonly string[]): string | null {
	const last = memo.split(MEMO_SEPARATOR).at(-1)?.trim() ?? '';
	if (last === '') return null;
	if (!ranked.includes(last)) return last;
	const used = new Set(memoPhrases(memo));
	return ranked.some((phrase) => !used.has(phrase) && phrase.includes(last)) ? last : null;
}

/** 今のメモ欄に出す候補。メモに入っている言葉は出さない。打ちかけがあれば、その文字を含むものに絞る。 */
export function memoSuggestions(
	ranked: readonly string[],
	memo: string,
	limit = MEMO_SUGGESTION_LIMIT
): string[] {
	const used = new Set(memoPhrases(memo));
	const typing = typingPhrase(memo, ranked);
	return ranked
		.filter((phrase) => !used.has(phrase))
		.filter((phrase) => typing === null || phrase.includes(typing))
		.slice(0, limit);
}

/** 候補をメモ欄に入れた後の値。打ちかけがあれば置き換え、無ければ「、」を挟んで後ろに足す。 */
export function applyMemoSuggestion(
	ranked: readonly string[],
	memo: string,
	phrase: string
): string {
	const phrases = memoPhrases(memo);
	if (typingPhrase(memo, ranked) !== null) phrases.pop();
	return [...phrases, phrase].join(MEMO_SEPARATOR);
}

/** 直近 `MEMO_SUGGESTION_DAYS` 日の記録から、候補の言葉をよく使う順に取る。取れなければ候補を出さない。 */
export async function fetchMemoPhrases(timeZone: string): Promise<string[]> {
	const today = localToday(timeZone);
	const from = new Date(today);
	from.setDate(from.getDate() - (MEMO_SUGGESTION_DAYS - 1));
	const result = await unwrapRequest(
		client.GET('/api/v1/records', {
			params: { query: { from: formatDateOnly(from), to: formatDateOnly(today) } }
		})
	);
	return result.ok ? rankMemoPhrases(result.data) : [];
}
