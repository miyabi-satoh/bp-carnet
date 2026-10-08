<script lang="ts">
	import LegalPage from '$lib/components/legal-page.svelte';
	import * as m from '$lib/paraglide/messages.js';
</script>

<LegalPage document="terms" title={m.terms_title()} />
