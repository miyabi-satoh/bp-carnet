<script lang="ts">
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
</script>

<!-- 未ログインで開く画面 (サインアップ・パスワードの設定と再設定) のカードの下に置く、ログインへ戻るリンク。 -->
<a
	href={resolve('/login')}
	class="mt-4 inline-flex min-h-11 items-center text-sm text-accent-foreground hover:text-primary"
>
	{m.common_back_to_login_link()}
</a>
