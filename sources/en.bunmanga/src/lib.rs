#![no_std]
use aidoku::{
	Chapter, HomeComponent, HomeComponentValue, HomeLayout, Link, Manga, MangaWithChapter, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::string::StripPrefixOrSelf,
	imports::{
		html::{Document, Element},
		net::Request,
		std::{current_date, parse_date},
	},
	prelude::*,
};
use madara::{Impl, Madara, Params};

const BASE_URL: &str = "https://bunmanga.com";

/// The browse orderings the home shelves are built from.
const HOME_ROWS: [(&str, &str, bool); 4] = [
	("Relevance", "relevance", false),
	("Top Rated", "rating", true),
	("Trending", "trending", true),
	("Latest", "latest", false),
];

fn browse_url(order: &str) -> String {
	format!("{BASE_URL}/?s=&post_type=wp-manga&m_orderby={order}")
}

/// Resolves the stamps the listing cards carry.
///
/// The cards write them relatively (`2 days ago`), while the new-tag badge
/// holds an absolute date in its title attribute.
fn listing_date(text: &str) -> Option<i64> {
	let trimmed = text.trim();
	if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("new") {
		return None;
	}

	let lowered = trimmed.to_lowercase();
	let mut words = lowered.trim_end_matches("ago").split_whitespace();
	if let Some(amount) = words.next().and_then(|word| word.parse::<i64>().ok())
		&& let Some(unit) = words.next()
	{
		let seconds = if unit.starts_with("second") {
			Some(1)
		} else if unit.starts_with("min") {
			Some(60)
		} else if unit.starts_with("hour") {
			Some(3600)
		} else if unit.starts_with("day") {
			Some(86400)
		} else if unit.starts_with("week") {
			Some(604800)
		} else if unit.starts_with("month") {
			Some(2592000)
		} else if unit.starts_with("year") {
			Some(31536000)
		} else {
			None
		};
		if let Some(seconds) = seconds {
			return Some(current_date() - amount * seconds);
		}
	}

	parse_date(trimmed, "yyyy-MM-dd HH:mm:ss")
		.or_else(|| parse_date(trimmed, "yyyy-MM-dd"))
		.or_else(|| parse_date(trimmed, "MMMM d, yyyy"))
		.or_else(|| parse_date(trimmed, "MMM d, yyyy"))
}

fn cover_of(element: &Element, selector: &str) -> Option<String> {
	let image = element.select_first(selector)?;
	image
		.attr("abs:data-src")
		.or_else(|| image.attr("abs:data-lazy-src"))
		.or_else(|| image.attr("abs:src"))
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

/// Reads the latest chapter a listing card advertises, with its release time.
fn listing_chapter(item: &Element) -> Option<Chapter> {
	let link = item.select_first(".latest-chap .chapter a, .chapter-item .chapter a")?;
	let href = link.attr("abs:href")?;
	let title = link.text().map(|text| text.trim().to_string())?;
	if title.is_empty() {
		return None;
	}

	let stamp = item
		.select_first(".meta-item.post-on, .chapter-item")
		.and_then(|holder| {
			holder
				.select_first(".c-new-tag")
				.and_then(|tag| tag.attr("title"))
				.or_else(|| holder.text())
		});

	Some(Chapter {
		key: href.strip_prefix_or_self(BASE_URL).into(),
		chapter_number: chapter_number(&title),
		date_uploaded: stamp.as_deref().and_then(listing_date),
		title: Some(title),
		url: Some(href),
		..Default::default()
	})
}

fn card_manga(item: &Element, title_selector: &str, image_selector: &str) -> Option<Manga> {
	let link = item.select_first(title_selector)?;
	let href = link.attr("abs:href")?;
	let title = link
		.text()
		.or_else(|| link.attr("title"))
		.map(|text| text.trim().to_string())
		.filter(|text| !text.is_empty())?;
	Some(Manga {
		key: href.strip_prefix_or_self(BASE_URL).into(),
		title,
		cover: cover_of(item, image_selector),
		url: Some(href),
		..Default::default()
	})
}

fn browse_cards(document: &Document) -> Vec<Manga> {
	document
		.select(".c-tabs-item__content, .page-item-detail")
		.map(|items| {
			items
				.filter_map(|item| card_manga(&item, ".post-title a", ".item-thumb img, img"))
				.collect()
		})
		.unwrap_or_default()
}

struct BunManga;

impl Impl for BunManga {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			..Default::default()
		}
	}

	fn get_home(&self, _params: &Params) -> Result<HomeLayout> {
		// The home page carries two of the rows and each ordering is its own
		// browse query, so they all go out together.
		let mut urls = vec![BASE_URL.to_string(), browse_url("views")];
		urls.extend(HOME_ROWS.iter().map(|(_, order, _)| browse_url(order)));
		let requests = urls
			.iter()
			.map(|url| Request::get(url).map_err(Into::into))
			.collect::<Result<Vec<_>>>()?;
		let documents: Vec<Option<Document>> = Request::send_all(requests)
			.into_iter()
			.map(|response| response.ok().and_then(|response| response.get_html().ok()))
			.collect();

		let mut components: Vec<HomeComponent> = Vec::new();

		if let Some(document) = documents.get(1).and_then(Option::as_ref) {
			let entries: Vec<Manga> = browse_cards(document).into_iter().take(10).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Popular".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					},
				});
			}
		}

		if let Some(home) = documents.first().and_then(Option::as_ref) {
			// The daily chart is one of several recent-manga widgets, told apart
			// by its heading.
			let top_daily: Vec<Link> = home
				.select(".widget-manga-recent")
				.map(|widgets| {
					widgets
						.filter(|widget| {
							widget
								.select_first(".heading")
								.and_then(|heading| heading.text())
								.is_some_and(|heading| {
									heading.trim().eq_ignore_ascii_case("top daily")
								})
						})
						.filter_map(|widget| widget.select(".popular-item-wrap"))
						.flatten()
						.filter_map(|item| {
							card_manga(&item, ".widget-title a", ".popular-img img, img")
						})
						.map(Into::into)
						.collect()
				})
				.unwrap_or_default();
			if !top_daily.is_empty() {
				components.push(HomeComponent {
					title: Some("Top Daily".into()),
					subtitle: None,
					value: HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries: top_daily,
						listing: None,
					},
				});
			}

			let latest: Vec<MangaWithChapter> = home
				.select(".c-blog-listing .page-item-detail")
				.map(|items| {
					items
						.filter_map(|item| {
							Some(MangaWithChapter {
								manga: card_manga(&item, ".post-title a", ".item-thumb img, img")?,
								chapter: listing_chapter(&item)?,
							})
						})
						.collect()
				})
				.unwrap_or_default();
			if !latest.is_empty() {
				components.push(HomeComponent {
					title: Some("Latest Updates".into()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: None,
						entries: latest,
						listing: None,
					},
				});
			}
		}

		for ((title, _, ranked), document) in HOME_ROWS.iter().zip(documents.into_iter().skip(2)) {
			let Some(document) = document else { continue };
			let entries: Vec<Link> = browse_cards(&document)
				.into_iter()
				.map(Into::into)
				.collect();
			if entries.is_empty() {
				continue;
			}
			components.push(HomeComponent {
				title: Some((*title).into()),
				subtitle: None,
				value: if *ranked {
					HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries,
						listing: None,
					}
				} else {
					HomeComponentValue::Scroller {
						entries,
						listing: None,
					}
				},
			});
		}

		Ok(HomeLayout { components })
	}
}

register_source!(
	Madara<BunManga>,
	Home,
	ListingProvider,
	DeepLinkHandler,
	ImageRequestProvider
);
