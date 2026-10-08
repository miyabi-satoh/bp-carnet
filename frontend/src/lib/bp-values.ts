import * as m from '$lib/paraglide/messages.js';
import { inRange } from '$lib/utils';

/** バックエンドの `src/validation.rs` と同じ値域。送信前に弾き、サーバー側の 422 で往復するのを避ける。 */
const SYSTOLIC_RANGE = [60, 260] as const;
const DIASTOLIC_RANGE = [30, 180] as const;
const PULSE_RANGE = [30, 220] as const;

/** 上下の血圧と脈拍。`undefined` は未入力。 */
export type BpValues = {
	systolic: number | undefined;
	diastolic: number | undefined;
	pulse: number | undefined;
};

/** 値域エラーの対象になる入力欄。 */
export type BpField = keyof BpValues;

/** 値域エラー1件。`fields` は、記録フォームがエラーの欄だけを強調するために使う。 */
export type BpValueIssue = { fields: BpField[]; message: string };

/** 値域チェック。未入力の値は見ない (必須かどうかは呼び出し元が判断する)。上下の大小関係は、
 * 両方が値域内のときだけ見る。 */
export function bpValueIssues({ systolic, diastolic, pulse }: BpValues): BpValueIssue[] {
	const issues: BpValueIssue[] = [];
	if (systolic !== undefined && !inRange(systolic, SYSTOLIC_RANGE)) {
		issues.push({
			fields: ['systolic'],
			message: m.record_error_systolic_range({ min: SYSTOLIC_RANGE[0], max: SYSTOLIC_RANGE[1] })
		});
	}
	if (diastolic !== undefined && !inRange(diastolic, DIASTOLIC_RANGE)) {
		issues.push({
			fields: ['diastolic'],
			message: m.record_error_diastolic_range({ min: DIASTOLIC_RANGE[0], max: DIASTOLIC_RANGE[1] })
		});
	}
	if (
		issues.length === 0 &&
		systolic !== undefined &&
		diastolic !== undefined &&
		systolic <= diastolic
	) {
		issues.push({
			fields: ['systolic', 'diastolic'],
			message: m.record_error_systolic_not_greater()
		});
	}
	if (pulse !== undefined && !inRange(pulse, PULSE_RANGE)) {
		issues.push({
			fields: ['pulse'],
			message: m.record_error_pulse_range({ min: PULSE_RANGE[0], max: PULSE_RANGE[1] })
		});
	}
	return issues;
}
