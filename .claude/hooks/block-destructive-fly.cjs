#!/usr/bin/env node
// Fly.io の Machine・Volume・app を消す・止める・台数を変えるコマンドを、Claude が実行する前に止める。
// 本番も検証用も同じコマンドで触れるうえ、消したものは戻せない (2026-09-28、検証用の Machine を `fly scale count 0` で消した)。
// 使うときは、ユーザーが `! <command>` で自分で実行する。
// Windows でも動くよう、シェルや jq に頼らず Node だけで書く (Bash と PowerShell の両方を見る)。
'use strict';

const fs = require('node:fs');

const FLY = /(^|[\s("'&])(fly|flyctl)(\.exe)?(?=\s|$)/i;
const DESTRUCTIVE =
	/\s(scale\s+count|(machines?|m)\s+(destroy|kill|stop|suspend|cordon)|(volumes?|vol|v)\s+destroy|(apps?\s+)?destroy)(?=\s|$)/i;

/** ; && || | 改行 で区切った1つずつのコマンド。行の継続 (Bash の \、PowerShell の `) は先につなぐ。 */
function commandSegments(command) {
	return command.replace(/[\\`]\r?\n/g, ' ').split(/&&|\|\||[;|\r\n]/);
}

function blockedSegment(command) {
	return commandSegments(command).find((segment) => FLY.test(segment) && DESTRUCTIVE.test(segment));
}

function main() {
	let input;
	try {
		input = JSON.parse(fs.readFileSync(0, 'utf8'));
	} catch {
		return;
	}
	const command = input.tool_input?.command;
	if (typeof command !== 'string') return;
	const segment = blockedSegment(command);
	if (!segment) return;
	process.stdout.write(
		JSON.stringify({
			hookSpecificOutput: {
				hookEventName: 'PreToolUse',
				permissionDecision: 'deny',
				permissionDecisionReason: [
					'Fly.io の Machine・Volume・app を消す・止める・台数を変えるコマンドは止めています (.claude/hooks/block-destructive-fly.cjs)。',
					`止めたコマンド: ${segment.trim()}`,
					'必要なら、何のために実行するかをユーザーに伝え、ユーザー自身に `! <command>` で実行してもらってください。'
				].join('\n')
			}
		})
	);
}

// settings.json から `node -e "require(...).main()"` で呼ぶ。PowerShell では $CLAUDE_PROJECT_DIR が展開されないため、
// パスはシェルでなく Node の process.env から組み立てる。
module.exports = { blockedSegment, main };
