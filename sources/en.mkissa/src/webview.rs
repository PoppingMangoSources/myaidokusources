use crate::models::*;
use aidoku::{
	Result,
	alloc::{String, Vec},
	imports::js::{WebView, WebViewUserScript},
	imports::net::Request,
	imports::std::sleep,
	prelude::*,
};

const RESULT_TOKEN: &str = "__AIDOKU_MKISSA_PAGES__";
const WAIT_TOKEN: &str = "__AIDOKU_MKISSA_WAIT__";

/// Captures the decoded `chapterPages` payload before the reader consumes it.
fn capture_script() -> String {
	format!(
		"(() => {{
	if (window['{RESULT_TOKEN}']) return;
	window['{RESULT_TOKEN}'] = {{ data: '', done: false, settled: false }};
	const state = window['{RESULT_TOKEN}'];
	const finish = (payload) => {{
		if (state.settled) return;
		state.settled = true;
		state.data = payload;
		state.done = true;
	}};
	const capture = (value) => {{
		try {{
			if (!value || typeof value !== 'object') return;
			const chapterPages = value.chapterPages || (value.data && value.data.chapterPages);
			if (chapterPages) {{
				finish(JSON.stringify({{ chapterPages }}));
				return;
			}}
			const pages = value.edges && Array.isArray(value.edges) ? value : null;
			if (pages && pages.edges.some((edge) => edge && Array.isArray(edge.pictureUrls) && edge.pictureUrls.length)) {{
				finish(JSON.stringify({{ chapterPages: pages }}));
			}}
		}} catch (_) {{}}
	}};
	const originalParse = JSON.parse;
	JSON.parse = new Proxy(originalParse, {{
		apply(target, thisArg, args) {{
			const parsed = Reflect.apply(target, thisArg, args);
			capture(parsed);
			return parsed;
		}}
	}});
	if (window.Response) {{
		const originalJson = Response.prototype.json;
		Response.prototype.json = function () {{
			return originalJson.call(this).then((parsed) => {{
				capture(parsed);
				return parsed;
			}});
		}};
	}}
	const originalFetch = window.fetch;
	if (originalFetch) {{
		window.fetch = function (...args) {{
			return originalFetch.apply(this, args).then((response) => {{
				try {{
					response.clone().text().then((raw) => {{
						try {{ capture(JSON.parse(raw)); }} catch (_) {{}}
					}});
				}} catch (_) {{}}
				return response;
			}});
		}};
	}}
	if (window.XMLHttpRequest) {{
		const originalOpen = XMLHttpRequest.prototype.open;
		XMLHttpRequest.prototype.open = function (...args) {{
			this.addEventListener('load', function () {{
				try {{ capture(JSON.parse(this.responseText)); }} catch (_) {{}}
			}});
			return originalOpen.apply(this, args);
		}};
	}}

	// Fallback: once the reader renders its pages, read the image srcs straight
	// off the document. This does not depend on the shape of the api response,
	// only on the pages the reader actually shows.
	let last = -1;
	let stable = 0;
	const scrape = () => {{
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
	let step = 0;
	const tick = setInterval(() => {{
		if (state.settled) {{ clearInterval(tick); return; }}
		// Walk down the page so every lazily mounted image gets a chance to set
		// its src, then return to the top for the next pass.
		const height = document.body.scrollHeight || 0;
		const y = ((step % 10) / 9) * height;
		window.scrollTo(0, y);
		step++;
		const urls = scrape();
		stable = urls.length > 0 && urls.length === last ? stable + 1 : 0;
		last = urls.length;
		if (urls.length > 0 && stable >= 3) {{
			clearInterval(tick);
			finish(JSON.stringify({{ chapterPages: {{ edges: [{{ pictureUrls: urls }}] }} }}));
		}}
	}}, 300);

	setTimeout(() => {{
		clearInterval(tick);
		if (state.settled) return;
		const urls = scrape();
		if (urls.length > 0) {{
			finish(JSON.stringify({{ chapterPages: {{ edges: [{{ pictureUrls: urls }}] }} }}));
		}} else {{
			finish('');
		}}
	}}, 90000);
}})()"
	)
}

/// Loads a chapter in a background web view and returns its page urls.
pub fn page_urls_via_webview(manga_id: &str, chapter: &str) -> Result<Vec<String>> {
	let urls = collect_pages(manga_id, chapter)?;
	if urls.is_empty() {
		bail!("The reader did not produce any pages");
	}
	let quality = crate::settings::image_quality();
	Ok(urls
		.into_iter()
		.map(|url| crate::parsers::apply_image_quality(&url, &quality))
		.collect())
}

fn collect_pages(manga_id: &str, chapter: &str) -> Result<Vec<String>> {
	let reader_url = format!("{DOMAIN}/manga/{manga_id}/chapter-{chapter}-sub/");

	// One real navigation, nothing else. The web view renders the reader page
	// itself, so a Cloudflare check shows up here in the reader and, once the
	// reader solves it, its scripts fetch the pages. Fetching the page over a
	// separate request first only made Cloudflare prompt a second time.
	let web_view = WebView::new();
	let mut user_script = WebViewUserScript::new(capture_script());
	user_script.at_document_end = false;
	user_script.for_main_frame_only = false;
	web_view.add_user_script(user_script)?;
	web_view.load(
		Request::get(&reader_url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8"),
	)?;
	// The Cloudflare check reloads the page once it passes, so wait for the real
	// reader document to settle before relying on the collector it carries.
	web_view.wait_for_load();

	// Long enough for the reader to clear a Cloudflare check and then fetch its
	// pages before giving up.
	let mut result = String::new();
	for _ in 0..90 {
		if let Ok(value) = web_view.eval(&format!(
			"(() => {{
				const state = window['{RESULT_TOKEN}'];
				return state && state.done ? state.data : '{WAIT_TOKEN}';
			}})()"
		)) {
			result = value;
			if result != WAIT_TOKEN && !result.is_empty() {
				break;
			}
		}
		sleep(1);
	}
	if result == WAIT_TOKEN || result.is_empty() {
		return Ok(Vec::new());
	}
	let parsed: ApiPagesResponse = match serde_json::from_str(&result) {
		Ok(parsed) => parsed,
		Err(_) => return Ok(Vec::new()),
	};
	let Some(pages) = parsed
		.chapter_pages
		.or_else(|| parsed.data.and_then(|data| data.chapter_pages))
	else {
		return Ok(Vec::new());
	};
	Ok(crate::parsers::parse_page_urls(&pages, "original"))
}

#[derive(serde::Deserialize)]
struct ApiPagesData {
	#[serde(rename = "chapterPages")]
	chapter_pages: Option<ChapterPages>,
}

#[derive(serde::Deserialize)]
struct ApiPagesResponse {
	#[serde(rename = "chapterPages")]
	chapter_pages: Option<ChapterPages>,
	data: Option<ApiPagesData>,
}
