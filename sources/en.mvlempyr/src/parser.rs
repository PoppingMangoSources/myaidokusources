use crate::models::*;
use aidoku::{
	Manga,
	alloc::{String, Vec, string::ToString},
	imports::html::Document,
	prelude::*,
};

const TAG_MODULUS: u64 = 1_999_999_997;

/// The chapter feed keys posts by `7^code mod 1999999997`.
pub fn chapter_tag_id(code: i64) -> u64 {
	let mut result: u64 = 1;
	let mut base: u64 = 7 % TAG_MODULUS;
	let mut exponent = code.max(0) as u64;
	while exponent > 0 {
		if exponent % 2 == 1 {
			result = (result * base) % TAG_MODULUS;
		}
		base = (base * base) % TAG_MODULUS;
		exponent /= 2;
	}
	result
}

pub fn parse_novel_code(html: &Document) -> Option<i64> {
	html.select_first("#novel-code")
		.and_then(|el| el.text())
		.and_then(|text| text.trim().parse::<i64>().ok())
}

fn clean(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_novel_details(html: &Document, slug: &str, code: Option<i64>) -> Manga {
	let title = html
		.select_first("h1.novel-title, .novel-title2")
		.and_then(|el| el.text())
		.map(|t| clean(&t))
		.filter(|t| !t.is_empty())
		.unwrap_or_else(|| slug.to_string());

	let description = html
		.select(".synopsis p")
		.map(|els| {
			els.filter_map(|el| el.text())
				.map(|t| clean(&t))
				.filter(|t| !t.is_empty())
				.collect::<Vec<_>>()
				.join("\n\n")
		})
		.filter(|t| !t.is_empty())
		.or_else(|| {
			html.select_first(".synopsis")
				.and_then(|el| el.text())
				.map(|t| clean(&t))
		})
		.filter(|t| !t.is_empty());

	let author = html.select(".textwrapper").and_then(|els| {
		els.filter_map(|el| el.text())
			.map(|t| clean(&t))
			.find_map(|text| {
				text.strip_prefix("Author:")
					.map(|name| name.trim().to_string())
					.filter(|name| !name.is_empty())
			})
	});

	let genres: Vec<String> = html
		.select(".genre-tags")
		.map(|els| {
			let mut seen: Vec<String> = Vec::new();
			for text in els.filter_map(|el| el.text()).map(|t| clean(&t)) {
				if !text.is_empty() && !seen.iter().any(|g| g == &text) {
					seen.push(text);
				}
			}
			seen
		})
		.unwrap_or_default();

	let cover = html
		.select_first("img.novel-image, img.novel-image2")
		.and_then(|img| img.attr("abs:src"))
		.or_else(|| code.map(|code| format!("{ASSETS_URL}/images/600/{code}.webp")));

	let status_text = html
		.select_first(".novelstatustextlarge")
		.and_then(|el| el.text())
		.unwrap_or_default();

	let novel = Novel {
		name: title,
		slug: slug.to_string(),
		code: code.unwrap_or_default(),
		genres,
		author,
		status: Some(status_text),
		..Default::default()
	};

	let mut manga = Manga::from(novel);
	manga.description = description;
	if let Some(cover) = cover {
		manga.cover = Some(cover);
	}
	manga.url = Some(format!("{DOMAIN}/novel/{slug}"));
	manga
}

/// Pulls the readable chapter body out of the reader page.
pub fn parse_chapter_text(html: &Document) -> String {
	let container = html.select_first("#chapter");
	let paragraphs = container
		.as_ref()
		.and_then(|el| el.select("p"))
		.map(|els| {
			els.filter_map(|el| el.text())
				.map(|t| clean(&t))
				.filter(|t| !t.is_empty())
				.collect::<Vec<_>>()
				.join("\n\n")
		})
		.unwrap_or_default();

	if !paragraphs.is_empty() {
		return paragraphs;
	}

	container
		.and_then(|el| el.text())
		.map(|t| t.trim().to_string())
		.unwrap_or_default()
}
