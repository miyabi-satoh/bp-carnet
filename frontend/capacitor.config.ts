import type { CapacitorConfig } from '@capacitor/cli';

// iPhone・iPad のアプリ (docs/mobile-app.md)。
// ADR: 設定はプラグインを入れる package.json と同じここに置き、iOS のプロジェクトだけを `mobile/` に出す。
// Capacitor は `cap sync` を走らせる側の package.json からプラグインを探すため、設定を `mobile/` に置くと
// プラグインを2か所に書くことになる。
const config: CapacitorConfig = {
	appId: 'com.amiiby.bpcarnet',
	appName: 'BP Carnet',
	webDir: 'build',
	ios: {
		path: '../mobile/ios'
	}
};

export default config;
