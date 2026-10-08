import { describe, expect, it } from 'vitest';
import { filterAdminUsers } from './admin-search';

const users = [
	{ username: 'kyoko@example.com', displayName: '佐藤 京子' },
	{ username: 'saburo@example.com', displayName: null },
	{ username: 'Admin' }
];

describe('filterAdminUsers', () => {
	it('空なら全員を残す', () => {
		expect(filterAdminUsers(users, '  ')).toEqual(users);
	});

	it('ユーザー ID か名前に含まれる人だけを残す', () => {
		expect(filterAdminUsers(users, 'saburo')).toEqual([users[1]]);
		expect(filterAdminUsers(users, '京子')).toEqual([users[0]]);
		expect(filterAdminUsers(users, 'example')).toEqual([users[0], users[1]]);
	});

	it('全角・半角と大文字・小文字を区別しない', () => {
		expect(filterAdminUsers(users, 'ＡＤＭＩＮ')).toEqual([users[2]]);
		expect(filterAdminUsers(users, 'KYOKO')).toEqual([users[0]]);
	});
});
