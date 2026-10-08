import { error } from '@sveltejs/kit';
import { findHelpTopic, loadHelpTopicContent } from '$lib/help';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params }) => {
	const topic = findHelpTopic(params.slug);
	if (!topic) error(404);
	return { topic, Content: await loadHelpTopicContent(topic) };
};
