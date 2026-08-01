#![no_std]
use aidoku::{
	Chapter, ContentRating, FilterItem, FilterValue, HomeComponent, HomeComponentValue, HomeLayout,
	Manga, MangaPageResult, MangaWithChapter, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::{string::StripPrefixOrSelf, uri::QueryParameters},
	imports::defaults::defaults_get,
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

fn parse_home_cards(url: &str, selector: &str) -> Result<Vec<Manga>> {
	let html = Request::get(url)?.html()?;
	let hide_nsfw = !show_nsfw();
	Ok(html
		.select(selector)
		.map(|items| {
			items
				.filter_map(|item| {
					let link = item
						.select_first("a[href*='/manga/']")
						.or_else(|| item.select_first("a"))?;
					let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
					let title = item
						.select_first(".name, .title, h3, h2")
						.and_then(|el| el.text())
						.or_else(|| link.attr("title"))?;
					let tags: Vec<String> = item
						.select(".genres a, .genres span, .genres-content a")
						.map(|tags| tags.filter_map(|tag| tag.text()).collect())
						.unwrap_or_default();
					let content_rating = rating_for(&tags);
					if hide_nsfw && content_rating == ContentRating::NSFW {
						return None;
					}
					let image = item.select_first("img")?;
					Some(Manga {
						key: href.strip_prefix_or_self(base_url()).into(),
						title: title.trim().to_string(),
						cover: image
							.attr("abs:data-src")
							.or_else(|| image.attr("abs:src"))
							.or_else(|| image.attr("src")),
						tags: (!tags.is_empty()).then_some(tags),
						content_rating,
						url: Some(href),
						..Default::default()
					})
				})
				.collect()
		})
		.unwrap_or_default())
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

	fn get_home(&self, params: &Params) -> Result<HomeLayout> {
		let base = base_url();
		let mut components = Vec::new();

		for (title, path, selector, featured, ranked) in [
			(
				"Top of the Week",
				"/top/week",
				".book-detailed-item",
				true,
				false,
			),
			("Hot Updates", "/home", ".trending-item", false, true),
			("Trending", "/top/day", ".book-detailed-item", true, false),
			(
				"Most Talked About",
				"/top/reviews",
				".book-detailed-item",
				false,
				true,
			),
			(
				"Most Viewed",
				"/az-list",
				".book-detailed-item",
				false,
				true,
			),
			(
				"Editor's Choice",
				"/top/comments",
				".book-detailed-item",
				false,
				true,
			),
		] {
			let Ok(mut mangas) = parse_home_cards(&format!("{base}{path}"), selector) else {
				continue;
			};
			if mangas.is_empty() {
				continue;
			}
			let descriptive = featured || title == "Editor's Choice";
			if descriptive {
				mangas = mangas
					.into_iter()
					.take(8)
					.map(|manga| {
						self.get_manga_update(params, manga.clone(), true, false)
							.unwrap_or(manga)
					})
					.collect();
			}
			let value = if descriptive {
				HomeComponentValue::BigScroller {
					entries: mangas,
					auto_scroll_interval: Some(6.0),
				}
			} else if ranked {
				HomeComponentValue::MangaList {
					ranking: true,
					page_size: Some(10),
					entries: mangas.into_iter().map(Into::into).collect(),
					listing: None,
				}
			} else {
				HomeComponentValue::Scroller {
					entries: mangas.into_iter().map(Into::into).collect(),
					listing: None,
				}
			};
			components.push(HomeComponent {
				title: Some(title.into()),
				subtitle: None,
				value,
			});
		}

		if let Ok(html) = Request::get(format!("{base}/home"))?.html()
			&& let Some(items) = html.select(".book-item")
		{
			let latest: Vec<MangaWithChapter> = items
				.filter_map(|item| {
					let link = item.select_first("a[href*='/manga/']")?;
					let href = link.attr("abs:href").or_else(|| link.attr("href"))?;
					let chapter = item.select_first("a[href*='chapter']")?;
					let chapter_href = chapter.attr("abs:href").or_else(|| chapter.attr("href"))?;
					let chapter_title = chapter.text()?;
					let image = item.select_first("img")?;
					Some(MangaWithChapter {
						manga: Manga {
							key: href.strip_prefix_or_self(&base).into(),
							title: item
								.select_first(".name, .title")
								.and_then(|el| el.text())
								.or_else(|| link.attr("title"))?,
							cover: image.attr("abs:data-src").or_else(|| image.attr("abs:src")),
							url: Some(href),
							..Default::default()
						},
						chapter: Chapter {
							key: chapter_href.strip_prefix_or_self(&base).into(),
							chapter_number: first_number(&chapter_title),
							title: Some(chapter_title),
							url: Some(chapter_href),
							..Default::default()
						},
					})
				})
				.collect();
			if !latest.is_empty() {
				components.insert(
					2.min(components.len()),
					HomeComponent {
						title: Some("Latest Updates".into()),
						subtitle: None,
						value: HomeComponentValue::MangaChapterList {
							page_size: None,
							entries: latest,
							listing: None,
						},
					},
				);
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
