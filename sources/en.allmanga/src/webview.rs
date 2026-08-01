use crate::models::*;
use aidoku::{
	Result,
	alloc::{String, Vec},
	imports::js::WebView,
	imports::net::Request,
	prelude::*,
};

const RESULT_TOKEN: &str = "__AIDOKU_ALLMANGA_PAGES__";

/// Watches the reader for its page images.
///
/// The reader fetches and renders its pages itself, so rather than racing its
/// scripts the collector polls the document and only reports once the image
/// count has held steady, which means lazy loading has caught up.
fn collector_script() -> String {
	format!(
		"(() => {{
	window['{RESULT_TOKEN}'] = {{ data: null, isDone: false, isAbort: false }};
	const state = window['{RESULT_TOKEN}'];
	let last = -1;
	let stable = 0;

	const collect = () => {{
		const seen = {{}};
		const out = [];
		const images = document.querySelectorAll('img');
		for (let i = 0; i < images.length; i++) {{
			const src = images[i].currentSrc || images[i].getAttribute('src') || '';
			if (!src || src.indexOf('data:') === 0) continue;
			if (src.indexOf('youtube-anime.com') === -1) continue;
			if (src.indexOf('aln.youtube-anime.com') !== -1) continue;
			if (seen[src]) continue;
			seen[src] = 1;
			out.push(src);
		}}
		return out;
	}};

	const finish = (urls) => {{
		state.data = JSON.stringify(urls);
		state.isDone = true;
	}};

	const tick = setInterval(() => {{
		// Nudge the reader so lazily mounted pages render.
		window.scrollTo(0, document.body.scrollHeight);
		const urls = collect();
		stable = urls.length > 0 && urls.length === last ? stable + 1 : 0;
		last = urls.length;
		if (urls.length > 0 && stable >= 3) {{
			clearInterval(tick);
			finish(urls);
		}}
	}}, 250);

	setTimeout(() => {{
		clearInterval(tick);
		if (state.isDone) return;
		const urls = collect();
		if (urls.length > 0) {{
			finish(urls);
		}} else {{
			state.isAbort = true;
		}}
	}}, 20000);

	return '';
}})()"
	)
}

/// Loads a chapter in a background web view and returns its page urls.
pub fn page_urls_via_webview(manga_id: &str, chapter: &str) -> Result<Vec<String>> {
	let reader_url = format!("{DOMAIN}/manga/{manga_id}/chapter-{chapter}-sub");

	// Load the real page so its scripts run against the site's own origin.
	let web_view = WebView::new();
	web_view.load_blocking(
		Request::get(&reader_url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8"),
	)?;

	web_view.eval(&collector_script())?;

	while web_view.eval(&format!(
		"(() => {{ return window['{RESULT_TOKEN}'].isDone ? 'true' : 'false'; }})()"
	))? == "false"
	{
		if web_view.eval(&format!(
			"(() => {{ return window['{RESULT_TOKEN}'].isAbort ? 'true' : 'false'; }})()"
		))? == "true"
		{
			bail!("The reader did not produce any pages");
		}
	}

	let result = web_view.eval(&format!(
		"(() => {{ return window['{RESULT_TOKEN}'].data || ''; }})()"
	))?;

	let urls: Vec<String> = serde_json::from_str(&result).unwrap_or_default();
	Ok(urls
		.into_iter()
		.map(|url| crate::parsers::apply_image_quality(&url, &crate::settings::image_quality()))
		.collect())
}
