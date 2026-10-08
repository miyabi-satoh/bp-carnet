// `E2E_KEEP_DATA=1` の実行の始めに、前回の控えを消す (Playwright の globalSetup、→ kept-data.ts)。
import { rmSync } from 'node:fs';
import { KEPT_DATA_FILE } from './kept-data';

export default function resetKeptData(): void {
	rmSync(KEPT_DATA_FILE, { force: true });
}
