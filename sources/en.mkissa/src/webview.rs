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

fn is_parser_key_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn parse_quoted_key(input: &str) -> Option<String> {
	let bytes = input.as_bytes();
	let quote = *bytes.first()?;
	if quote != b'\'' && quote != b'"' {
		return None;
	}
	let end = bytes[1..].iter().position(|byte| *byte == quote)? + 1;
	let key = &input[1..end];
	(!key.is_empty() && key.bytes().all(is_parser_key_byte)).then(|| String::from(key))
}

fn resolve_parser_key_assignment(script: &str, target: &str) -> Option<String> {
	let mut offset = 0;
	while let Some(relative) = script[offset..].find(target) {
		let start = offset + relative;
		let end = start + target.len();
		let before_is_key = start > 0 && is_parser_key_byte(script.as_bytes()[start - 1]);
		let after_is_key = end < script.len() && is_parser_key_byte(script.as_bytes()[end]);
		if !before_is_key && !after_is_key {
			let rest = script[end..].trim_start();
			if rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>") {
				let expression = rest[1..].split(';').next()?;
				let bytes = expression.as_bytes();
				let mut key = String::new();
				let mut found_literal = false;
				let mut index = 0;
				while index < bytes.len() {
					let quote = bytes[index];
					if quote != b'\'' && quote != b'"' {
						index += 1;
						continue;
					}
					let literal_start = index + 1;
					index = literal_start;
					while index < bytes.len() && bytes[index] != quote {
						index += 1;
					}
					if index >= bytes.len() {
						return None;
					}
					let literal = &expression[literal_start..index];
					if literal.bytes().all(is_parser_key_byte) {
						key.push_str(literal);
						found_literal = true;
					}
					index += 1;
				}
				if found_literal && !key.is_empty() {
					return Some(key);
				}
			}
		}
		offset = end;
	}
	None
}

/// Finds the generated window property that the reader uses to pin an untouched
/// `JSON.parse`. The key may be written directly or assembled from string
/// literals in a nearby variable assignment.
fn pinned_parser_key(html: &str) -> Option<String> {
	let mut offset = 0;
	while let Some(relative) = html[offset..].find("defineProperty") {
		let call_start = offset + relative;
		let script_start = html[..call_start].rfind("<script")?;
		let body_start = script_start + html[script_start..].find('>')? + 1;
		let script_end = call_start + html[call_start..].find("</script>")?;
		let script = &html[body_start..script_end];
		if !script.contains("JSON") {
			offset = call_start + "defineProperty".len();
			continue;
		}

		let after_name = &html[call_start + "defineProperty".len()..script_end];
		let open = after_name.find('(')?;
		let mut rest = after_name[open + 1..].trim_start();
		if !rest.starts_with("window") {
			offset = call_start + "defineProperty".len();
			continue;
		}
		rest = rest["window".len()..].trim_start();
		if !rest.starts_with(',') {
			offset = call_start + "defineProperty".len();
			continue;
		}
		rest = rest[1..].trim_start();

		if let Some(key) = parse_quoted_key(rest) {
			return Some(key);
		}

		let target_len = rest.bytes().take_while(|byte| is_parser_key_byte(*byte)).count();
		if target_len > 0 {
			let target = &rest[..target_len];
			if let Some(key) = resolve_parser_key_assignment(script, target) {
				return Some(key);
			}
		}
		offset = call_start + "defineProperty".len();
	}
	None
}

/// Hooks the reader's own decode and drives the app to the chapter.
///
/// The manga page is loaded first, then a link to the chapter is clicked so the
/// site's router navigates to the reader client-side and fetches its pages. The
/// generated pinned-parser key is claimed before the site's scripts run, while
/// `chapterPages` is still captured from `Response.json` / `JSON.parse` as a
/// fallback.
fn capture_script(chapter_path: &str, pinned_key: Option<&str>) -> String {
	let chapter_path =
		serde_json::to_string(chapter_path).unwrap_or_else(|_| String::from("\"\""));
	let pin = pinned_key
		.and_then(|key| serde_json::to_string(key).ok())
		.map(|key| {
			format!(
				"try {{
		Object.defineProperty(window, {key}, {{
			value: captureParse,
			writable: false,
			configurable: false,
			enumerable: false
		}});
	}} catch (_) {{}}"
			)
		})
		.unwrap_or_default();

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
	const capture = (parsed, raw) => {{
		try {{
			if (parsed && (parsed.chapterPages || (parsed.data && parsed.data.chapterPages))) {{
				finish(raw);
			}}
		}} catch (_) {{}}
	}};

	const originalJson = Response.prototype.json;
	Response.prototype.json = function () {{
		return originalJson.call(this).then((data) => {{
			capture(data, JSON.stringify(data));
			return data;
		}});
	}};

	const originalParse = JSON.parse;
	const captureParse = new Proxy(originalParse, {{
		apply(target, thisArg, args) {{
			const result = Reflect.apply(target, thisArg, args);
			capture(result, args[0]);
			return result;
		}}
	}});
	JSON.parse = captureParse;
	{pin}

	function triggerChapterNav() {{
		const a = document.createElement('a');
		a.href = a.dataset.href = {chapter_path};
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
	let parser_key = pinned_parser_key(&html);

	// Load the cleared manga page, then navigate to the chapter client-side. The
	// site's router fetches the pages from its api without another page load, so
	// Cloudflare is not asked a second time.
	let web_view = WebView::new();
	let mut user_script =
		WebViewUserScript::new(capture_script(&chapter_path, parser_key.as_deref()));
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
