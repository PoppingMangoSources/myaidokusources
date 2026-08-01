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

/// Captures the decoded `chapterPages` response before the reader consumes it.
fn capture_script() -> String {
	format!(
		"(() => {{
	if (window['{RESULT_TOKEN}']) return;
	window['{RESULT_TOKEN}'] = {{ data: '', done: false }};
	const state = window['{RESULT_TOKEN}'];
	let settled = false;
	const finish = (value) => {{
		if (settled) return;
		settled = true;
		state.data = value || '';
		state.done = true;
	}};
	const capture = (parsed, raw) => {{
		try {{
			const pages = parsed && (parsed.chapterPages || (parsed.data && parsed.data.chapterPages));
			if (pages && pages.edges && pages.edges.length) finish(raw);
		}} catch (_) {{}}
	}};
	const originalParse = JSON.parse;
	JSON.parse = new Proxy(originalParse, {{
		apply(target, thisArg, args) {{
			const parsed = Reflect.apply(target, thisArg, args);
			capture(parsed, typeof args[0] === 'string' ? args[0] : JSON.stringify(parsed));
			return parsed;
		}}
	}});
	const originalJson = Response.prototype.json;
	Response.prototype.json = function () {{
		return originalJson.call(this).then((parsed) => {{
			capture(parsed, JSON.stringify(parsed));
			return parsed;
		}});
	}};
	const originalFetch = window.fetch;
	if (originalFetch) window.fetch = function (...args) {{
		return originalFetch.apply(this, args).then((response) => {{
			try {{
				response.clone().text().then((raw) => {{
					try {{ capture(originalParse(raw), raw); }} catch (_) {{}}
				}});
			}} catch (_) {{}}
			return response;
		}});
	}};
	const originalOpen = XMLHttpRequest.prototype.open;
	XMLHttpRequest.prototype.open = function (...args) {{
		this.addEventListener('load', function () {{
			try {{
				const raw = this.responseText;
				capture(originalParse(raw), raw);
			}} catch (_) {{}}
		}});
		return originalOpen.apply(this, args);
	}};
	setTimeout(() => finish(''), 18000);
}})()"
	)
}

/// Loads a chapter in a background web view and returns its page urls.
///
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
	let reader_url = format!("{DOMAIN}/manga/{manga_id}/chapter-{chapter}-sub");
	let web_view = WebView::new();
	let mut user_script = WebViewUserScript::new(capture_script());
	user_script.at_document_end = false;
	user_script.for_main_frame_only = true;
	web_view.add_user_script(user_script)?;
	// `load` preserves WebView cookies and page navigation without the unbounded
	// wait used by `load_blocking`; the poll below supplies the hard deadline.
	web_view.load(
		Request::get(&reader_url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8"),
	)?;

	let mut result = String::new();
	for _ in 0..20 {
		if let Ok(value) = web_view.eval(&format!(
			"(() => {{
				const state = window['{RESULT_TOKEN}'];
				return state && state.done ? state.data : '{WAIT_TOKEN}';
			}})()"
		)) {
			result = value;
			if result != WAIT_TOKEN {
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
