import prettier from 'eslint-config-prettier';
import path from 'node:path';
import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import { defineConfig, includeIgnoreFile } from 'eslint/config';
import globals from 'globals';
import ts from 'typescript-eslint';

const gitignorePath = path.resolve(import.meta.dirname, '.gitignore');

export default defineConfig(
	includeIgnoreFile(gitignorePath),
	{
		// shadcn-svelte が生成するコンポーネント (registry からの取得物なので対象外)
		ignores: ['src/lib/components/ui/**']
	},
	{
		// openapi-typescript が生成する型定義 (just api-types で再生成されるので対象外)
		ignores: ['src/lib/api/schema.d.ts']
	},
	js.configs.recommended,
	ts.configs.recommended,
	svelte.configs.recommended,
	prettier,
	svelte.configs.prettier,
	{
		languageOptions: { globals: { ...globals.browser, ...globals.node } },
		rules: {
			// typescript-eslint strongly recommend that you do not use the no-undef lint rule on TypeScript projects.
			// see: https://typescript-eslint.io/troubleshooting/faqs/eslint/#i-get-errors-from-the-no-undef-rule-about-global-variables-not-being-defined-even-though-there-are-no-typescript-errors
			'no-undef': 'off'
		}
	},
	{
		// projectService (型情報を使う型検査ルール) を .ts にも適用し、no-deprecated を
		// 効かせる ($app/stores・svelte/store 系・createEventDispatcher 等の @deprecated
		// なAPIを機械的に検知するため)。
		files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js', '**/*.ts'],
		languageOptions: {
			parserOptions: {
				projectService: true,
				extraFileExtensions: ['.svelte'],
				parser: ts.parser
			}
		},
		rules: {
			'@typescript-eslint/no-deprecated': 'warn'
		}
	},
	{
		// Override or add rule settings here, such as:
		// 'svelte/button-has-type': 'error'
		rules: {}
	}
);
