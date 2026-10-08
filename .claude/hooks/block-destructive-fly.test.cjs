// node --test .claude/hooks/block-destructive-fly.test.cjs
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { blockedSegment } = require('./block-destructive-fly.cjs');

test('Machine・Volume・app を消す・止める・台数を変えるコマンドを止める', () => {
	for (const command of [
		'fly scale count 0 --app bp-carnet-staging --yes',
		'curl x; fly m destroy 123 -a bp-carnet',
		'flyctl volumes destroy vol_1',
		'fly vol destroy vol_1 --yes',
		'fly apps destroy bp-carnet',
		'fly destroy bp-carnet',
		'fly machine stop 123',
		'fly machines kill 123',
		'fly -a bp-carnet machine suspend 123',
		'fly scale \\\ncount 0 --app bp-carnet',
		// PowerShell の書き方
		'fly scale `\r\ncount 0 --app bp-carnet',
		'fly.exe scale count 0 --app bp-carnet',
		'& fly machine destroy 123',
		'$env:FLY_APP="bp-carnet"; FLY Volumes Destroy vol_1'
	]) {
		assert.ok(blockedSegment(command), command);
	}
});

test('読み取りや本番への載せ方など、ふだん使うコマンドは通す', () => {
	for (const command of [
		'fly status --app bp-carnet',
		'fly machines list -a bp-carnet',
		'fly volumes list --app bp-carnet-staging',
		'fly logs --app bp-carnet',
		'fly scale show --app bp-carnet',
		'fly releases --app bp-carnet',
		'just deploy',
		'just deploy-staging',
		'echo butterfly scale count'
	]) {
		assert.equal(blockedSegment(command), undefined, command);
	}
});
