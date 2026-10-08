// メモの候補。候補は直近の記録すべてから作るため、ほかのテストの記録が混ざらない
// 使い捨てユーザーで流す。
import { createRecordsByApi, fetchTimeZone, type NewRecord } from './api-helpers';
import { addDays, todayIn } from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { waitForApiResponse } from './helpers';
import { HomePage } from './pages/home-page';

test.use({ storageState: NO_LOGIN });

/** 今日から `daysAgo` 日前の朝の記録。 */
function recordDaysAgo(today: string, daysAgo: number, memo: string): NewRecord {
	return {
		localMeasuredAt: `${addDays(today, -daysAgo)}T07:00`,
		systolic: 120,
		diastolic: 80,
		memo
	};
}

test('直近90日のメモの言葉を、よく使う順に出し、タップで「、」を挟んで足す', async ({ page }) => {
	const today = todayIn(await fetchTimeZone(page.request));
	await createRecordsByApi(page.request, [
		recordDaysAgo(today, 1, '起床後、薬を飲む前'),
		recordDaysAgo(today, 2, '起床後'),
		recordDaysAgo(today, 3, '頭痛'),
		// 期間の端。89日前は数え、90日前は数えない。
		recordDaysAgo(today, 89, '寝る前'),
		recordDaysAgo(today, 90, '古い言葉')
	]);
	const home = new HomePage(page);
	await home.open();
	const { fields } = await home.openAddSheet();

	// 回数の多い順、同じ回数なら最近使った順。
	await expect(fields.memoSuggestions).toHaveText(['起床後', '薬を飲む前', '頭痛', '寝る前']);

	await fields.memoSuggestions.filter({ hasText: '起床後' }).click();
	await expect(fields.memo).toHaveValue('起床後');
	// メモに入っている言葉は出さない。
	await expect(fields.memoSuggestions).toHaveText(['薬を飲む前', '頭痛', '寝る前']);

	await fields.memoSuggestions.filter({ hasText: '頭痛' }).click();
	await expect(fields.memo).toHaveValue('起床後、頭痛');
	await expect(fields.memoSuggestions).toHaveText(['薬を飲む前', '寝る前']);
});

test('打ちかけの言葉があれば、その文字を含む候補に絞り、タップで置き換える', async ({ page }) => {
	const today = todayIn(await fetchTimeZone(page.request));
	await createRecordsByApi(page.request, [
		recordDaysAgo(today, 1, '起床後、薬を飲む前'),
		recordDaysAgo(today, 2, '薬を飲んだ後、頭痛'),
		recordDaysAgo(today, 3, '頭痛薬')
	]);
	const home = new HomePage(page);
	await home.open();
	const { fields } = await home.openAddSheet();
	await expect(fields.memoSuggestions).toHaveCount(5);

	await fields.memo.fill('起床後、薬');
	await expect(fields.memoSuggestions).toHaveText(['薬を飲む前', '薬を飲んだ後', '頭痛薬']);
	await fields.memoSuggestions.filter({ hasText: '薬を飲んだ後' }).click();
	await expect(fields.memo).toHaveValue('起床後、薬を飲んだ後');

	// どの候補にも当たらなければ、候補の欄ごと出さない。
	await fields.memo.fill('めまい');
	await expect(fields.memoSuggestions).toHaveCount(0);

	// 候補と同じ言葉でも、ほかの候補に含まれていれば打ちかけとみなす。
	await fields.memo.fill('頭痛');
	await expect(fields.memoSuggestions).toHaveText(['頭痛薬']);
	await fields.memoSuggestions.filter({ hasText: '頭痛薬' }).click();
	await expect(fields.memo).toHaveValue('頭痛薬');
});

test('候補は5件までにする', async ({ page }) => {
	const today = todayIn(await fetchTimeZone(page.request));
	await createRecordsByApi(page.request, [recordDaysAgo(today, 1, '一、二、三、四、五、六')]);
	const home = new HomePage(page);
	await home.open();
	const { fields } = await home.openAddSheet();
	await expect(fields.memoSuggestions).toHaveText(['一', '二', '三', '四', '五']);
});

test('画面の幅より長い言葉は、チップを幅に収めて「…」で省き、タップで全文を入れる', async ({
	page
}) => {
	// 「、」で区切らずに書いた長い文。
	const long = `朝から少しだるい${'。'.repeat(40)}`;
	const today = todayIn(await fetchTimeZone(page.request));
	await createRecordsByApi(page.request, [recordDaysAgo(today, 1, long)]);
	const home = new HomePage(page);
	await home.open();
	const sheet = await home.openAddSheet();
	const chip = sheet.fields.memoSuggestions.first();
	await expect(chip).toHaveText(long);

	expect(await sheet.fields.memoSuggestionFitsInWidth(chip)).toBe(true);
	expect(await sheet.fields.isMemoSuggestionTruncated(chip)).toBe(true);

	await chip.click();
	await expect(sheet.fields.memo).toHaveValue(long);
});

test('登録したメモの言葉は、次に記録フォームを開いたときに候補に出る', async ({ page }) => {
	const home = new HomePage(page);
	await home.open();
	const sheet = await home.openAddSheet();
	// 記録が無ければ候補は出ない。
	await expect(sheet.fields.memoSuggestions).toHaveCount(0);
	await sheet.fields.fill({ systolic: '120', diastolic: '80', memo: '散歩の後' });
	const created = waitForApiResponse(page, 'POST', '/records');
	await sheet.createButton.click();
	expect((await created).status()).toBe(201);
	await expect(sheet.root).toBeHidden();

	const next = await home.openAddSheet();
	await expect(next.fields.memoSuggestions).toHaveText(['散歩の後']);
});
