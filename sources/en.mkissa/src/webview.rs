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

/// Hooks the reader's own decode and drives the app to the chapter.
///
/// The manga page is loaded first, then a link to the chapter is clicked so the
/// site's router navigates to the reader client-side and fetches its pages. The
/// `chapterPages` payload is captured off `Response.json` / `JSON.parse` before
/// the reader consumes it. This mirrors the community keiyoushi source.
fn capture_script(chapter_path: &str) -> String {
	format!(
		"(function () {{
	if (window['{RESULT_TOKEN}']) return;
	window['{RESULT_TOKEN}'] = {{ data: '', done: false }};
	const state = window['{RESULT_TOKEN}'];
	const finish = (payload) => {{
		if (state.done) return;
		state.data = payload;
		state.done = true;
	}};

	const originalJson = Response.prototype.json;
	Response.prototype.json = function () {{
		return originalJson.call(this).then((data) => {{
			try {{ if (data && data.chapterPages) finish(JSON.stringify(data)); }} catch (_) {{}}
			return data;
		}});
	}};

	const originalParse = JSON.parse;
	JSON.parse = new Proxy(originalParse, {{
		apply(target, thisArg, args) {{
			const result = Reflect.apply(target, thisArg, args);
			try {{ if (result && result.chapterPages) finish(args[0]); }} catch (_) {{}}
			return result;
		}}
	}});

	function triggerChapterNav() {{
		const a = document.createElement('a');
		a.href = a.dataset.href = '{chapter_path}';
		document.body.append(a);
		a.click();
	}}

	let attempts = 0;
	function check() {{
		if (state.done) return;
		if (document.querySelector('[data-href]')) {{
			triggerChapterNav();
		}} else if (attempts < 300) {{
			attempts++;
			setTimeout(check, 50);
		}} else {{
			triggerChapterNav();
		}}
	}}
	check();
}})()"
	)
}

/// Loads a chapter through the reader and returns its page urls.
pub fn page_urls_via_webview(manga_id: &str, chapter: &str) -> Result<Vec<String>> {
	let pages = collect_pages(manga_id, chapter)?;
	let quality = crate::settings::image_quality();
	let urls = crate::parsers::parse_page_urls(&pages, &quality);
	if urls.is_empty() {
		bail!("The reader did not produce any pages");
	}
	Ok(urls)
}

fn collect_pages(manga_id: &str, chapter: &str) -> Result<ChapterPages> {
	let manga_url = format!("{DOMAIN}/manga/{manga_id}");
	let chapter_path = format!("/manga/{manga_id}/chapter-{chapter}-sub");

	// Fetch the manga page over an ordinary request. Aidoku clears Cloudflare
	// here — silently, or through the captcha sheet the app shows — and stores
	// the clearance cookie. If it still comes back challenged, ask the reader to
	// retry once the check is solved.
	let response = Request::get(&manga_url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
		.send()?;
	let status = response.status_code();
	if status == 403 || status == 503 {
		bail!("Solve the Cloudflare check, then retry");
	} else if status >= 400 {
		bail!("The reader returned HTTP {status}");
	}
	let html = response.get_string()?;

	// Load the cleared manga page, then navigate to the chapter client-side. The
	// site's router fetches the pages from its api without another page load, so
	// Cloudflare is not asked a second time.
	let web_view = WebView::new();
	let mut user_script = WebViewUserScript::new(capture_script(&chapter_path));
	user_script.at_document_end = false;
	user_script.for_main_frame_only = false;
	web_view.add_user_script(user_script)?;
	web_view.load_html_blocking(&html, Some(&manga_url))?;

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
		bail!("The reader did not produce any pages");
	}

	let parsed: ApiPagesResponse =
		serde_json::from_str(&result).or_else(|_| bail!("Failed to read the reader pages"))?;
	parsed
		.chapter_pages
		.or_else(|| parsed.data.and_then(|data| data.chapter_pages))
		.ok_or_else(|| error!("The reader did not produce any pages"))
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
