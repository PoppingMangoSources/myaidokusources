use crate::models::*;
use aidoku::{
	Result,
	alloc::{String, Vec},
	imports::js::WebView,
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
	setTimeout(() => finish(''), 30000);
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

	// Fetch the reader through an ordinary request first. The app intercepts a
	// Cloudflare challenge on this request and shows it in the page, then keeps
	// the clearance cookie; a headless web view load would hide the challenge
	// and simply come back empty. This is the pattern the community comix source
	// uses to keep Cloudflare in front of the user.
	let response = Request::get(&reader_url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
		.send()?;
	let status = response.status_code();
	if status == 403
		&& response
			.get_header("cf-mitigated")
			.is_some_and(|value| value == "challenge")
	{
		bail!("Cloudflare is asking for a check — solve it here, then tap retry");
	} else if status >= 400 {
		bail!("The reader returned HTTP {status}");
	}

	// Load the html that was just fetched rather than re-navigating to the url:
	// a fresh navigation inside the headless view would slip past the app's
	// Cloudflare handling again. The capture hooks are prepended into the head
	// so they install before the reader's own scripts run, exactly as the
	// Paperback source and the community comix source do.
	let html = response.get_string()?;
	let script = format!("<head><script>{}</script>", capture_script());
	let patched = if html.contains("<head>") {
		html.replacen("<head>", &script, 1)
	} else {
		format!("{script}</head>{html}")
	};

	let web_view = WebView::new();
	web_view.load_html_blocking(&patched, Some(&reader_url))?;

	let mut result = String::new();
	for _ in 0..60 {
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
