use crate::models::*;
use aidoku::{
	Chapter, ContentRating, MangaStatus,
	alloc::{String, Vec, string::ToString},
	imports::std::parse_date,
	prelude::*,
};

/// Decodes a small set of common HTML entities.
pub fn decode_entities(input: &str) -> String {
	input
		.replace("&amp;", "&")
		.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&#39;", "'")
		.replace("&apos;", "'")
		.replace("&nbsp;", " ")
}

pub fn parse_thumbnail_url(thumb: Option<&str>) -> String {
	let trimmed = thumb.map(str::trim).unwrap_or("");
	if trimmed.is_empty() {
		return format!("{THUMBNAIL_CDN}?w=250");
	}
	if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
		return trimmed.to_string();
	}
	format!("{THUMBNAIL_CDN}{}?w=250", trimmed.trim_start_matches('/'))
}

pub fn content_rating_for_genres(genres: &[String]) -> ContentRating {
	let lowered: Vec<String> = genres.iter().map(|g| g.trim().to_lowercase()).collect();
	if lowered
		.iter()
		.any(|g| g == "adult" || g == "hentai" || g == "smut" || g == "yaoi")
	{
		return ContentRating::NSFW;
	}
	if lowered.iter().any(|g| g == "ecchi" || g == "mature") {
		return ContentRating::Suggestive;
	}
	ContentRating::Safe
}

pub fn parse_status(status: Option<&str>) -> MangaStatus {
	let s = status.unwrap_or("").to_lowercase();
	if s.contains("releasing") || s.contains("ongoing") {
		MangaStatus::Ongoing
	} else if s.contains("finished") || s.contains("completed") {
		MangaStatus::Completed
	} else if s.contains("hiatus") {
		MangaStatus::Hiatus
	} else if s.contains("cancel") {
		MangaStatus::Cancelled
	} else {
		MangaStatus::Unknown
	}
}

/// Strips HTML tags from a description string and decodes basic entities.
pub fn strip_html(html: &str) -> String {
	let mut out = String::with_capacity(html.len());
	let mut in_tag = false;
	let normalized = html
		.replace("<br>", "\n")
		.replace("<br/>", "\n")
		.replace("<br />", "\n")
		.replace("</p>", "\n\n");
	for ch in normalized.chars() {
		match ch {
			'<' => in_tag = true,
			'>' => in_tag = false,
			_ => {
				if !in_tag {
					out.push(ch);
				}
			}
		}
	}
	decode_entities(out.trim())
}

pub fn apply_image_quality(url: &str, quality: &str) -> String {
	if quality == "original" {
		return url.to_string();
	}
	let without_scheme = url
		.strip_prefix("https://")
		.or_else(|| url.strip_prefix("http://"));
	match without_scheme {
		Some(rest) => {
			let path = rest.split('#').next().unwrap_or(rest);
			format!("{IMAGE_CDN}/{path}?w={quality}")
		}
		None => url.to_string(),
	}
}

pub fn value_to_string(value: &serde_json::Value) -> Option<String> {
	match value {
		serde_json::Value::String(s) => Some(s.clone()),
		serde_json::Value::Number(n) => Some(n.to_string()),
		_ => None,
	}
}

/// Converts the source's split date parts (month is zero-indexed) to a unix timestamp.
pub fn date_from_parts(parts: Option<&DateParts>) -> Option<i64> {
	let parts = parts?;
	let year = parts.year?;
	let month = parts.month.unwrap_or(0) + 1;
	let day = parts.date.unwrap_or(1);
	let hour = parts.hour.unwrap_or(0);
	let minute = parts.minute.unwrap_or(0);
	let second = parts.second.unwrap_or(0);
	let formatted = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}");
	parse_date(formatted, "yyyy-MM-dd HH:mm:ss")
}

/// Parses an ISO-8601 timestamp string to a unix timestamp.
pub fn parse_iso_date(raw: &str) -> Option<i64> {
	let trimmed = raw.trim();
	if trimmed.len() < 19 {
		return None;
	}
	parse_date(&trimmed[..19], "yyyy-MM-dd'T'HH:mm:ss")
}

/// Strips a leading `[season]` tag and `ep. N -` prefix from a chapter note,
/// returning an empty string when nothing meaningful remains.
fn chapter_title_from(notes: &str) -> String {
	let trimmed = notes.trim();
	let without_season = if trimmed.starts_with('[') {
		match trimmed.find(']') {
			Some(idx) => trimmed[idx + 1..].trim_start(),
			None => trimmed,
		}
	} else {
		trimmed
	};

	let title = strip_ep_prefix(without_season).trim();
	if title.chars().any(|c| c.is_ascii_alphabetic()) {
		title.to_string()
	} else {
		String::new()
	}
}

/// Removes a leading `ep`/`ep.`/`ep 12` style prefix (case-insensitive).
fn strip_ep_prefix(input: &str) -> &str {
	let lower = input.to_lowercase();
	if !lower.starts_with("ep") {
		return input;
	}
	let bytes = input.as_bytes();
	let mut i = 2;
	if i < bytes.len() && bytes[i] == b'.' {
		i += 1;
	}
	while i < bytes.len() && bytes[i] == b' ' {
		i += 1;
	}
	let digits_start = i;
	while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
		i += 1;
	}
	if i == digits_start {
		return input;
	}
	while i < bytes.len() && bytes[i] == b' ' {
		i += 1;
	}
	if i < bytes.len() && bytes[i] == b'-' {
		i += 1;
	}
	while i < bytes.len() && bytes[i] == b' ' {
		i += 1;
	}
	&input[i..]
}

/// Builds the chapter list from the numeric chapter ids and episode metadata.
pub fn parse_chapters(data: &ChaptersData, manga_key: &str) -> Vec<Chapter> {
	let sub: &[String] = data
		.manga
		.available_chapters_detail
		.as_ref()
		.and_then(|detail| detail.sub.as_deref())
		.unwrap_or(&[]);
	let infos: &[EpisodeInfo] = data.episode_infos.as_deref().unwrap_or(&[]);

	let mut chapters: Vec<Chapter> = sub
		.iter()
		.map(|num| {
			let info = infos
				.iter()
				.find(|e| value_to_string(&e.episode_id_num).as_deref() == Some(num.as_str()));
			let title = info
				.and_then(|i| i.notes.as_deref())
				.map(chapter_title_from)
				.filter(|t| !t.is_empty())
				.map(|t| decode_entities(&t));
			let date_uploaded = info
				.and_then(|i| i.upload_dates.as_ref())
				.and_then(|u| u.sub.as_deref())
				.and_then(parse_iso_date);
			Chapter {
				key: num.clone(),
				title,
				chapter_number: num.parse::<f32>().ok(),
				date_uploaded,
				url: Some(format!("{DOMAIN}/manga/{manga_key}/chapter-{num}-sub")),
				language: Some("en".into()),
				..Default::default()
			}
		})
		.collect();

	chapters.sort_by(|a, b| {
		b.chapter_number
			.partial_cmp(&a.chapter_number)
			.unwrap_or(core::cmp::Ordering::Equal)
	});
	chapters
}

fn is_absolute(url: &str) -> bool {
	url.starts_with("http://") || url.starts_with("https://")
}

/// Builds the ordered list of page image urls for a chapter.
pub fn parse_page_urls(pages: &ChapterPages, quality: &str) -> Vec<String> {
	if pages.edges.is_empty() {
		return Vec::new();
	}

	let edge = pages
		.edges
		.iter()
		.find(|e| {
			let has_absolute = e
				.picture_urls
				.as_deref()
				.unwrap_or(&[])
				.iter()
				.any(|p| p.url().is_some_and(is_absolute));
			has_absolute || e.picture_url_head.is_some()
		})
		.unwrap_or(&pages.edges[0]);

	let image_domain = match edge.picture_url_head.as_deref() {
		Some(server) if is_absolute(server) => format!("{}/", server.trim_end_matches('/')),
		Some(server) => format!("https://{}/", server.trim_end_matches('/')),
		None => DEFAULT_IMAGE_SERVER.to_string(),
	};

	edge.picture_urls
		.as_deref()
		.unwrap_or(&[])
		.iter()
		.filter_map(|p| p.url())
		.filter(|url| !url.is_empty())
		.map(|url| {
			if is_absolute(url) {
				url.to_string()
			} else {
				format!("{image_domain}{}", url.trim_start_matches('/'))
			}
		})
		.map(|url| apply_image_quality(&url, quality))
		.collect()
}
