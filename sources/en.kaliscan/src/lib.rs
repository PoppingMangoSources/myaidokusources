#![no_std]
use aidoku::{
	Chapter, ContentRating, FilterItem, FilterValue, HomeComponent, HomeComponentValue, HomeLayout,
	Manga, MangaPageResult, MangaWithChapter, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::{string::StripPrefixOrSelf, uri::QueryParameters},
	imports::defaults::defaults_get,
	imports::html::Document,
	imports::net::Request,
	prelude::*,
};
use madtheme::{Impl, MadTheme, Params};

const DEFAULT_BASE_URL: &str = "https://kaliscan.io";

const BASE_URL_KEY: &str = "baseUrl";
const SHOW_NSFW_KEY: &str = "showNSFW";

const ADULT_GENRES: &[&str] = &["adult", "hentai", "smut", "mature", "erotica", "18+"];
const SUGGESTIVE_GENRES: &[&str] = &["ecchi", "bl", "gl", "yaoi", "yuri", "harem"];

fn base_url() -> String {
	defaults_get::<String>(BASE_URL_KEY)
		.map(|url| url.trim().trim_end_matches('/').to_string())
		.filter(|url| url.starts_with("http"))
		.unwrap_or_else(|| DEFAULT_BASE_URL.into())
}

fn show_nsfw() -> bool {
	defaults_get::<bool>(SHOW_NSFW_KEY).unwrap_or(false)
}

fn rating_for(tags: &[String]) -> ContentRating {
	let lowered: Vec<String> = tags.iter().map(|t| t.trim().to_lowercase()).collect();
	if lowered.iter().any(|t| ADULT_GENRES.contains(&t.as_str())) {
		ContentRating::NSFW
	} else if lowered
		.iter()
		.any(|t| SUGGESTIVE_GENRES.contains(&t.as_str()))
	{
		ContentRating::Suggestive
	} else {
		ContentRating::Unknown
	}
}

fn first_number(text: &str) -> Option<f32> {
	let mut number = String::new();
	for ch in text.chars() {
		if ch.is_ascii_digit() || (ch == '.' && !number.is_empty()) {
			number.push(ch);
		} else if !number.is_empty() {
			break;
		}
	}
	number.trim_matches('.').parse().ok()
}

fn parse_home_cards(document: &Document, selector: &str, base: &str) -> Vec<Manga> {
	let hide_nsfw = !show_nsfw();
	document
		.select(selector)
		.map(|items| {
			items
				.filter_map(|item| {
					let link = item
						.select_first("a[href*='/manga/'], a[href^='manga/']")
						.or_else(|| item.select_first("a"))?;
					let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
					let title = item
						.select_first(".name, .title, .book-title, h3, h2")
						.and_then(|el| el.text())
						.or_else(|| link.attr("title"))?;
					let title = title.trim().to_string();
					if title.is_empty() {
						return None;
					}
					let tags: Vec<String> = item
						.select(".genres a, .genres span, .genres-content a, a[href*='/genre/']")
						.map(|tags| {
							tags.filter_map(|tag| tag.text())
								.map(|tag| tag.trim().to_string())
								.filter(|tag| !tag.is_empty())
								.collect()
						})
						.unwrap_or_default();
					let content_rating = rating_for(&tags);
					if hide_nsfw && content_rating == ContentRating::NSFW {
						return None;
					}
					let image = item.select_first("img")?;
					let description = item
						.select_first(".description, .summary, .excerpt, .book-summary, p")
						.and_then(|el| el.text())
						.map(|text| text.trim().to_string())
						.filter(|text| !text.is_empty());
					Some(Manga {
						key: href.strip_prefix_or_self(base).into(),
						title,
						cover: image
							.attr("abs:data-src")
							.or_else(|| image.attr("abs:src"))
							.or_else(|| image.attr("data-src"))
							.or_else(|| image.attr("src")),
						description,
						tags: (!tags.is_empty()).then_some(tags),
						content_rating,
						url: Some(href),
						..Default::default()
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

fn parse_latest(document: &Document, base: &str) -> Vec<MangaWithChapter> {
	let hide_nsfw = !show_nsfw();
	document
		.select(".book-item, .book-item-list, .latest-updates .item")
		.map(|items| {
			items
				.filter_map(|item| {
					let link = item
						.select_first("a[href*='/manga/'], a[href^='manga/']")
						.or_else(|| item.select_first("a"))?;
					let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
					let chapter = item.select_first("a[href*='chapter']")?;
					let chapter_href = chapter.attr("abs:href").or_else(|| chapter.attr("href"))?;
					let chapter_title = chapter.text()?.trim().to_string();
					let image = item.select_first("img")?;
					let tags: Vec<String> = item
						.select(".genres a, .genres span, a[href*='/genre/']")
						.map(|tags| {
							tags.filter_map(|tag| tag.text())
								.map(|tag| tag.trim().to_string())
								.filter(|tag| !tag.is_empty())
								.collect()
						})
						.unwrap_or_default();
					let content_rating = rating_for(&tags);
					if hide_nsfw && content_rating == ContentRating::NSFW {
						return None;
					}
					Some(MangaWithChapter {
						manga: Manga {
							key: href.strip_prefix_or_self(base).into(),
							title: item
								.select_first(".name, .title, .book-title")
								.and_then(|el| el.text())
								.or_else(|| link.attr("title"))?
								.trim()
								.to_string(),
							cover: image
								.attr("abs:data-src")
								.or_else(|| image.attr("abs:src"))
								.or_else(|| image.attr("data-src"))
								.or_else(|| image.attr("src")),
							tags: (!tags.is_empty()).then_some(tags),
							content_rating,
							url: Some(href),
							..Default::default()
						},
						chapter: Chapter {
							key: chapter_href.strip_prefix_or_self(base).into(),
							chapter_number: first_number(&chapter_title),
							title: Some(chapter_title),
							url: Some(chapter_href),
							..Default::default()
						},
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

struct KaliScan;

impl Impl for KaliScan {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: base_url().into(),
			..Default::default()
		}
	}

	fn get_home(&self, _params: &Params) -> Result<HomeLayout> {
		let base = base_url();
		let urls = [
			format!("{base}/top/week"),
			format!("{base}/home"),
			format!("{base}/top/day"),
			format!("{base}/top/reviews"),
			format!("{base}/top/comments"),
		];
		let requests: Vec<Request> = urls
			.iter()
			.map(|url| Request::get(url).map_err(Into::into))
			.collect::<Result<Vec<_>>>()?;
		let documents: Vec<Option<Document>> = Request::send_all(requests)
			.into_iter()
			.map(|response| response.ok().and_then(|response| response.get_html().ok()))
			.collect();
		let mut components = Vec::new();

		if let Some(document) = documents.first().and_then(Option::as_ref) {
			let entries = parse_home_cards(document, ".book-detailed-item", &base);
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Top of the Week".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					},
				});
			}
		}

		if let Some(document) = documents.get(1).and_then(Option::as_ref) {
			let hot = parse_home_cards(document, ".trending-item", &base);
			if !hot.is_empty() {
				components.push(HomeComponent {
					title: Some("Hot Updates".into()),
					subtitle: None,
					value: HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries: hot.into_iter().map(Into::into).collect(),
						listing: None,
					},
				});
			}
			let latest = parse_latest(document, &base);
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

		if let Some(document) = documents.get(2).and_then(Option::as_ref) {
			let entries = parse_home_cards(document, ".book-detailed-item", &base);
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Trending".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					},
				});
			}
		}

		for (index, title) in [(3, "Most Talked About"), (4, "Editor's Choice")] {
			if let Some(document) = documents.get(index).and_then(Option::as_ref) {
				let entries = parse_home_cards(document, ".book-detailed-item", &base);
				if entries.is_empty() {
					continue;
				}
				let value = if title == "Editor's Choice" {
					HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					}
				} else {
					HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries: entries.into_iter().map(Into::into).collect(),
						listing: None,
					}
				};
				components.push(HomeComponent {
					title: Some(title.into()),
					subtitle: None,
					value,
				});
			}
		}

		let genres = [
			("Action", "action"),
			("Adventure", "adventure"),
			("Comedy", "comedy"),
			("Drama", "drama"),
			("Fantasy", "fantasy"),
			("Isekai", "isekai"),
			("Romance", "romance"),
			("Shoujo", "shoujo"),
		]
		.into_iter()
		.map(|(title, id)| FilterItem {
			title: title.into(),
			values: Some(vec![FilterValue::MultiSelect {
				id: "genre[]".into(),
				included: vec![id.into()],
				excluded: Vec::new(),
			}]),
		})
		.collect();
		components.push(HomeComponent {
			title: Some("Genres".into()),
			subtitle: None,
			value: HomeComponentValue::Filters(genres),
		});

		Ok(HomeLayout { components })
	}

	/// Reimplemented so genres are read off the search cards, which lets NSFW
	/// entries be filtered out before they ever reach the listing.
	fn get_search_manga_list(
		&self,
		params: &Params,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let mut qs = QueryParameters::new();
		qs.push("page", Some(&page.to_string()));
		qs.push("q", query.as_deref());
		qs.push("status", Some("all"));

		for filter in filters {
			match filter {
				FilterValue::Sort { id, index, .. } => {
					let value = match index {
						0 => "views",
						1 => "updated_at",
						2 => "created_at",
						3 => "name",
						4 => "rating",
						_ => "views",
					};
					qs.push(&id, Some(value));
				}
				FilterValue::Select { id, value } => qs.set(&id, Some(&value)),
				FilterValue::MultiSelect { id, included, .. } => {
					for item in included {
						qs.push(&id, Some(&item));
					}
				}
				_ => {}
			}
		}

		let url = format!("{}/search?{qs}", params.base_url);
		let html = Request::get(url)?.html()?;
		let hide_nsfw = !show_nsfw();

		let entries: Vec<Manga> = html
			.select(".book-detailed-item")
			.map(|els| {
				els.filter_map(|el| {
					let link = el.select_first("a")?;
					let tags: Vec<String> = el
						.select(".genres a, .genres span, .genres-content a")
						.map(|genres| {
							genres
								.filter_map(|genre| genre.text())
								.map(|text| text.trim().to_string())
								.filter(|text| !text.is_empty())
								.collect()
						})
						.unwrap_or_default();
					let content_rating = rating_for(&tags);
					if hide_nsfw && content_rating == ContentRating::NSFW {
						return None;
					}
					Some(Manga {
						key: link
							.attr("href")?
							.strip_prefix_or_self(&params.base_url)
							.into(),
						title: link.attr("title")?,
						cover: el.select_first("img")?.attr("abs:data-src"),
						tags: (!tags.is_empty()).then_some(tags),
						content_rating,
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: html
				.select_first(".paginator > a.active + a:not([rel=next])")
				.is_some(),
		})
	}
}

register_source!(
	MadTheme<KaliScan>,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
