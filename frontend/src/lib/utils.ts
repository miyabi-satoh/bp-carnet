import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

/** `[min, max]` (両端を含む) に収まるか。バックエンドと値域を合わせる定数は、この形の
 * タプルで持つ (`bp-values.ts`・OCR上限の入力欄)。 */
export function inRange(value: number, [min, max]: readonly [number, number]): boolean {
	return value >= min && value <= max;
}

/** 10進整数の文字列表現として厳密に解釈できなければ `null`。前後の空白は許容する
 * (`Number()` 単独だと "0x80" や "1e2" も数値として通ってしまうため、ここで規定する)。
 * `$lib/import` のCSV行パースとも共有する。 */
export function parseStrictInt(raw: string): number | null {
	const trimmed = raw.trim();
	if (!/^-?\d+$/.test(trimmed)) return null;
	return Number(trimmed);
}

/** 空文字列 (空白のみを含む) なら `undefined` (未入力)、そうでなければ `parseStrictInt` と同じ
 * (不正な値なら `null`)。未入力と不正な入力を区別したいとき (CSVの任意列・記録フォーム) に使う。 */
export function parseOptionalStrictInt(raw: string): number | undefined | null {
	if (raw.trim() === '') return undefined;
	return parseStrictInt(raw);
}

/** 隠れたファイル入力のピッカーを開く。同じファイルを続けて選んでも change が発火するよう、
 * 開く前に value をリセットする。 */
export function openFilePicker(input: HTMLInputElement | null): void {
	if (!input) return;
	input.value = '';
	input.click();
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type WithoutChild<T> = T extends { child?: any } ? Omit<T, 'child'> : T;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type WithoutChildren<T> = T extends { children?: any } ? Omit<T, 'children'> : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & { ref?: U | null };
