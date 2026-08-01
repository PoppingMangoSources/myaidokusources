#![no_std]
use aidoku::{
	ContentRating, FilterValue, Manga, MangaPageResult, Result, Source,
	alloc::{String, Vec, string::ToString},
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

register_source!(MadTheme<KaliScan>, ImageRequestProvider, DeepLinkHandler);
