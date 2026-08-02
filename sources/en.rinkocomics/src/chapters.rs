use aidoku::{
	Chapter, Result,
	alloc::{String, Vec, string::ToString},
	helpers::{string::StripPrefixOrSelf, uri::encode_uri_component},
	imports::{
		html::{Document, Element, Html},
		net::Request,
		std::{current_date, parse_date},
	},
	prelude::*,
};
use serde::Deserialize;

/// Chapters live behind a paginated admin-ajax action rather than the usual
/// Madara chapter endpoint, so the whole list has to be walked by hand.
const AJAX_PATH: &str = "/wp-admin/admin-ajax.php";
const CHAPTER_SELECTOR: &str = "li.chapter, div.chapter, a.chapter-item";
const LOAD_MORE_SELECTOR: &str = "[data-comic-id]";
const LOCK_SUFFIX: &str = "#lock";

/// Stop walking after this many pages so a misbehaving endpoint cannot spin.
const MAX_PAGES: usize = 60;

#[derive(Deserialize)]
struct AjaxResponse {
	#[serde(default)]
	success: bool,
	#[serde(default)]
	data: Option<AjaxData>,
}

#[derive(Deserialize)]
struct AjaxData {
	#[serde(default)]
	html: Option<String>,
}

/// Reads the nonce out of the inline `comicworld_ajax` config object.
fn extract_nonce(body: &str) -> Option<&str> {
	let start = body.find("comicworld_ajax")?;
	let rest = &body[start..];
	let open = rest.find('{')?;
	let close = rest[open..].find('}')? + open;
	let object = &rest[open..=close];

	let after = &object[object.find("\"nonce\"")? + "\"nonce\"".len()..];
	let value = &after[after.find(':')? + 1..];
	let open = value.find('"')? + 1;
	let close = value[open..].find('"')? + open;
	Some(&value[open..close])
}

fn is_locked(element: &Element) -> bool {
	if element.attr("data-reason").is_some() {
		return true;
	}
	element
		.attr("class")
		.is_some_and(|class| class.contains("locked-chapter") || class.contains("is-locked"))
}

/// Resolves a relative date such as `3 days ago` against the current time.
fn relative_date(text: &str) -> Option<i64> {
	let lowered = text.trim().to_lowercase();
	if !lowered.ends_with("ago") {
		return None;
	}
	let mut parts = lowered.split_whitespace();
	let amount: i64 = parts.next()?.parse().ok()?;
	let unit = parts.next()?;
	let seconds = match unit.trim_end_matches('s') {
		"second" => 1,
		"minute" => 60,
		"hour" => 3600,
		"day" => 86400,
		"week" => 604800,
		"month" => 2592000,
		"year" => 31536000,
		_ => return None,
	};
	Some(current_date() - amount * seconds)
}

fn chapter_date(text: &str) -> Option<i64> {
	relative_date(text)
		.or_else(|| parse_date(text, "MMMM d, yyyy"))
		.or_else(|| parse_date(text, "MMM d, yyyy"))
		.or_else(|| parse_date(text, "yyyy-MM-dd"))
}

fn chapter_number(title: &str) -> Option<f32> {
	let mut number = String::new();
	for ch in title.chars() {
		if ch.is_ascii_digit() || (ch == '.' && !number.is_empty()) {
			number.push(ch);
		} else if !number.is_empty() {
			break;
		}
	}
	number.trim_matches('.').parse().ok()
}

/// Strips the `Chapter 12 - ` prefix so only the chapter's own name is left.
fn chapter_title(name: &str) -> Option<String> {
	let rest = name
		.trim()
		.strip_prefix_or_self("Chapter")
		.strip_prefix_or_self("chapter")
		.trim_start();
	let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
	let rest = rest.trim_start_matches([' ', '-', ':']).trim();
	(!rest.is_empty()).then(|| rest.to_string())
}

fn parse_batch(document: &Document, base_url: &str) -> Vec<Chapter> {
	document
		.select(CHAPTER_SELECTOR)
		.map(|elements| {
			elements
				.filter_map(|element| {
					let href = element
						.attr("abs:data-permalink")
						.or_else(|| element.attr("data-permalink"))
						.or_else(|| element.attr("abs:href"))
						.or_else(|| element.attr("href"))
						.map(|url| url.trim().to_string())
						.filter(|url| !url.is_empty() && url != "#");
					let post_id = element
						.attr("data-post-id")
						.map(|id| id.trim().to_string())
						.filter(|id| !id.is_empty());
					let (href, post_id) = match (href, post_id) {
						(None, None) => return None,
						pair => pair,
					};

					let name = element
						.select_first(".chapter-number")
						.and_then(|el| el.text())
						.or_else(|| element.select_first(".ch-name").and_then(|el| el.text()))
						.or_else(|| {
							element
								.select_first(".chapter-side-title")
								.and_then(|el| el.text())
						})
						.or_else(|| element.attr("data-title"))
						.or_else(|| element.select_first("label").and_then(|el| el.text()))
						.map(|name| name.trim().to_string())
						.unwrap_or_default();

					let locked = is_locked(&element);
					let key = match &href {
						Some(url) => url.strip_prefix_or_self(base_url).to_string(),
						// Locked chapters have no link of their own, so the post
						// id is the only stable handle they have.
						None => format!("locked-{}", post_id.unwrap_or_default()),
					};

					Some(Chapter {
						key: if locked {
							format!("{key}{LOCK_SUFFIX}")
						} else {
							key
						},
						chapter_number: chapter_number(&name),
						title: chapter_title(&name),
						date_uploaded: element
							.select_first(".chapter-date")
							.and_then(|el| el.text())
							.and_then(|text| chapter_date(&text)),
						url: href,
						locked,
						..Default::default()
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

/// Collects every chapter, walking the ajax endpoint past the first page.
pub fn parse_chapters(body: &str, document: &Document, base_url: &str) -> Result<Vec<Chapter>> {
	let mut chapters = parse_batch(document, base_url);

	let load_more = document.select_first(LOAD_MORE_SELECTOR);
	let comic_id = load_more
		.as_ref()
		.and_then(|el| el.attr("data-comic-id"))
		.map(|id| id.trim().to_string())
		.filter(|id| !id.is_empty());
	let nonce = extract_nonce(body).map(|nonce| nonce.to_string());

	if let (Some(comic_id), Some(nonce)) = (comic_id, nonce) {
		let mut offset = load_more
			.and_then(|el| el.attr("data-offset"))
			.and_then(|offset| offset.trim().parse::<usize>().ok())
			.unwrap_or(chapters.len());
		let url = format!("{base_url}{AJAX_PATH}");
		let comic_id = encode_uri_component(comic_id);
		let nonce = encode_uri_component(nonce);

		for _ in 0..MAX_PAGES {
			let body = format!(
				"action=load_more_chapters&nonce={nonce}&comic_id={comic_id}&offset={offset}"
			);
			let Ok(response) = Request::post(&url)?
				.header(
					"Content-Type",
					"application/x-www-form-urlencoded; charset=UTF-8",
				)
				.header("X-Requested-With", "XMLHttpRequest")
				.header("Referer", &format!("{base_url}/"))
				.body(body)
				.send()
			else {
				break;
			};
			let Ok(text) = response.get_string() else {
				break;
			};
			let Ok(parsed) = serde_json::from_str::<AjaxResponse>(&text) else {
				break;
			};
			if !parsed.success {
				break;
			}
			let Some(fragment) = parsed.data.and_then(|data| data.html) else {
				break;
			};
			if fragment.trim().is_empty() {
				break;
			}
			let Ok(document) = Html::parse_fragment_with_url(&fragment, base_url) else {
				break;
			};
			let batch = parse_batch(&document, base_url);
			if batch.is_empty() {
				break;
			}
			offset += batch.len();
			chapters.extend(batch);
		}
	}

	// Newest first, matching how the app expects chapter lists to arrive.
	chapters.sort_by(|a, b| {
		b.chapter_number
			.partial_cmp(&a.chapter_number)
			.unwrap_or(core::cmp::Ordering::Equal)
	});
	chapters.dedup_by(|a, b| a.key == b.key);
	Ok(chapters)
}
