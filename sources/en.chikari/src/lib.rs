#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::de::DeserializeOwned;

mod models;
mod settings;

use models::*;

fn api_request(url: &str) -> Result<Request> {
	Ok(Request::get(url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Origin", DOMAIN)
		.header("Accept", "application/json, text/plain, */*"))
}

fn api_get<T: DeserializeOwned>(url: &str) -> Result<T> {
	api_request(url)?.send()?.get_json_owned()
}

fn parse_iso_date(raw: &str) -> Option<i64> {
	let trimmed = raw.trim();
	if trimmed.len() < 19 {
		return None;
	}
	parse_date(&trimmed[..19], "yyyy-MM-dd'T'HH:mm:ss")
}

fn item_to_manga(item: SeriesItem) -> Manga {
	let cover = format_cover_url(&item.cover_url, 400);
	let status = status_from(&item.status);
	let viewer = viewer_for_type(&item.kind);
	let description = Some(item_summary(&item));
	let content_rating = if item.is_nsfw {
		ContentRating::NSFW
	} else {
		ContentRating::Safe
	};
	Manga {
		key: item.slug,
		title: item.title,
		cover: Some(cover),
		description,
		status,
		content_rating,
		viewer,
		..Default::default()
	}
}

fn item_to_link(item: SeriesItem) -> Link {
	let manga = item_to_manga(item);
	Link {
		title: manga.title.clone(),
		subtitle: manga.description.clone(),
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn format_count(value: i64) -> String {
	if value >= 1_000_000 {
		format!("{:.1}M", value as f64 / 1_000_000.0)
	} else if value >= 1_000 {
		format!("{:.1}K", value as f64 / 1_000.0)
	} else {
		value.to_string()
	}
}

fn item_summary(item: &SeriesItem) -> String {
	let mut parts = vec![item.kind.to_uppercase()];
	if item.chapter_count > 0 {
		parts.push(format!("{} chapters", item.chapter_count));
	}
	if let Some(rating) = item.rating.filter(|rating| *rating > 0.0) {
		parts.push(format!("★ {rating:.1}"));
	}
	if item.views > 0 {
		parts.push(format!("{} views", format_count(item.views)));
	}
	parts.join(" · ")
}

fn take_home_row(rows: &mut Vec<HomeRow>, slug: &str) -> Vec<SeriesItem> {
	rows.iter()
		.position(|row| row.slug == slug)
		.map(|index| rows.remove(index).items)
		.unwrap_or_default()
}

/// The rows the home screen is built from, in display order.
const HOME_ROWS: [HomeRowSpec; 7] = [
	HomeRowSpec::new("Popular Manga", "popular", None, Some("manga")),
	HomeRowSpec::new("Popular Manhwa", "popular", None, Some("manhwa")),
	HomeRowSpec::new("Popular Manhua", "popular", None, Some("manhua")),
	HomeRowSpec::new(
		"Most Bookmarked This Month",
		"most_bookmarked",
		Some("month"),
		None,
	),
	HomeRowSpec::new("Top Rated Manga", "top_rated", None, Some("manga")),
	HomeRowSpec::new("Top Rated Manhwa", "top_rated", None, Some("manhwa")),
	HomeRowSpec::new("Top Rated Manhua", "top_rated", None, Some("manhua")),
];

struct HomeRowSpec {
	title: &'static str,
	sort: &'static str,
	period: Option<&'static str>,
	kind: Option<&'static str>,
}

impl HomeRowSpec {
	const fn new(
		title: &'static str,
		sort: &'static str,
		period: Option<&'static str>,
		kind: Option<&'static str>,
	) -> Self {
		Self {
			title,
			sort,
			period,
			kind,
		}
	}

	/// The listing this row's "see all" opens.
	fn listing(&self) -> Listing {
		Listing {
			id: match self.kind {
				Some(kind) => format!("{}_{kind}", self.sort),
				None => self.sort.into(),
			},
			name: self.title.into(),
			..Default::default()
		}
	}

	fn url(&self) -> String {
		let types = self
			.kind
			.map(|kind| vec![kind.to_string()])
			.unwrap_or_default();
		series_url(self.sort, self.period, None, &types, &[], 0)
	}
}

fn series_url(
	sort: &str,
	period: Option<&str>,
	query: Option<&str>,
	types: &[String],
	statuses: &[String],
	offset: i32,
) -> String {
	let mut qs = QueryParameters::new();
	qs.push("sort", Some(sort));
	qs.push("adult", Some(&settings::adult().to_string()));
	qs.push(
		"content_rating",
		Some(&settings::content_ratings().join(",")),
	);
	qs.push("limit", Some(&PAGE_SIZE.to_string()));
	qs.push("offset", Some(&offset.to_string()));
	if let Some(period) = period {
		qs.push("period", Some(period));
	}
	if let Some(query) = query.filter(|q| !q.is_empty()) {
		qs.push("q", Some(query));
	}
	let types = if types.is_empty() {
		settings::content_types()
	} else {
		types.to_vec()
	};
	for kind in &types {
		qs.push("type", Some(kind));
	}
	for status in statuses {
		qs.push("status", Some(status));
	}
	format!("{API_URL}/series?{qs}")
}

fn sort_id(index: i32) -> &'static str {
	match index {
		1 => "top_rated",
		2 => "trending",
		3 => "updated",
		4 => "added",
		5 => "most_bookmarked",
		_ => "popular",
	}
}

fn fetch_series(
	sort: &str,
	period: Option<&str>,
	query: Option<&str>,
	types: &[String],
	statuses: &[String],
	offset: i32,
) -> Result<SeriesListResponse> {
	api_get(&series_url(sort, period, query, types, statuses, offset))
}

struct Chikari;

impl Source for Chikari {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default();
		let query = query.trim();

		if let Some(result) = resolve_url_query(query)? {
			return Ok(result);
		}

		let mut sort = String::from("popular");
		let mut period: Option<String> = None;
		let mut types: Vec<String> = Vec::new();
		let mut statuses: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = sort_id(index).into(),
				FilterValue::Select { id, value } if id == "sort" => sort = value,
				FilterValue::Select { id, value } if id == "period" => period = Some(value),
				FilterValue::MultiSelect { id, included, .. } if id == "type" => types = included,
				FilterValue::MultiSelect { id, included, .. } if id == "status" => {
					statuses = included
				}
				_ => {}
			}
		}

		let page = page.max(1);
		let offset = (page - 1) * PAGE_SIZE;
		let data = fetch_series(
			&sort,
			period.as_deref(),
			(!query.is_empty()).then_some(query),
			&types,
			&statuses,
			offset,
		)?;
		let next_offset = offset + data.items.len() as i32;
		let has_next_page = !data.items.is_empty() && next_offset < data.total;
		let entries = data.items.into_iter().map(item_to_manga).collect();
		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = manga.key.clone();

		if needs_details {
			let details: SeriesDetails = api_get(&format!("{API_URL}/series/{slug}"))?;

			manga.title = details.title;
			manga.cover = Some(format_cover_url(&details.cover_url, 600));
			if !details.description.is_empty() {
				manga.description = Some(details.description);
			}
			manga.status = status_from(&details.status);
			manga.viewer = viewer_for_type(&details.kind);
			manga.content_rating = if details.is_nsfw {
				ContentRating::NSFW
			} else {
				ContentRating::Safe
			};
			manga.url = Some(format!("{DOMAIN}/series/{slug}"));

			let authors: Vec<String> = details
				.authors
				.iter()
				.filter(|c| c.role == "author")
				.map(|c| c.name.clone())
				.filter(|n| !n.is_empty())
				.collect();
			let artists: Vec<String> = details
				.authors
				.iter()
				.filter(|c| c.role == "artist")
				.map(|c| c.name.clone())
				.filter(|n| !n.is_empty())
				.collect();
			if !authors.is_empty() {
				manga.authors = Some(authors);
			}
			if !artists.is_empty() {
				manga.artists = Some(artists);
			}

			let mut tags: Vec<String> = details
				.genres
				.into_iter()
				.map(|g| g.name)
				.filter(|n| !n.is_empty())
				.collect();
			tags.extend(
				details
					.tags
					.into_iter()
					.filter(|t| !t.is_spoiler)
					.map(|t| t.name)
					.filter(|n| !n.is_empty()),
			);
			if !tags.is_empty() {
				manga.tags = Some(tags);
			}

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let response: ChapterListResponse = api_get(&format!(
				"{API_URL}/series/{slug}/chapters?order=asc&limit=100000&offset=0"
			))?;
			manga.chapters = Some(parse_chapters(response.items, &slug));
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let response: ChapterDetailsResponse = api_get(&format!(
			"{API_URL}/series/{}/chapters/{}",
			manga.key, chapter.key
		))?;
		if response.pages.is_empty() {
			bail!("No pages found");
		}
		Ok(response
			.pages
			.into_iter()
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

fn parse_chapters(items: Vec<ChapterItem>, slug: &str) -> Vec<Chapter> {
	let mut chapters: Vec<Chapter> = items
		.into_iter()
		.enumerate()
		.map(|(index, item)| {
			let number = item.number;
			let key = chapter_token(number);
			let title = {
				let trimmed = item.title.trim();
				if number.is_none() {
					Some(if trimmed.is_empty() {
						"Oneshot".into()
					} else {
						trimmed.to_string()
					})
				} else if trimmed.is_empty() {
					None
				} else {
					Some(trimmed.to_string())
				}
			};
			let volume_number = item.volume.parse::<f32>().ok().filter(|v| *v > 0.0);
			let language = if item.lang.is_empty() {
				"en".into()
			} else {
				item.lang
			};
			Chapter {
				url: Some(format!("{DOMAIN}/series/{slug}/{key}")),
				key,
				title,
				chapter_number: Some(number.unwrap_or((index + 1) as f32)),
				volume_number,
				date_uploaded: parse_iso_date(&item.created_at),
				language: Some(language),
				..Default::default()
			}
		})
		.collect();
	chapters.reverse();
	chapters
}

fn resolve_url_query(query: &str) -> Result<Option<MangaPageResult>> {
	let Some(idx) = query.find("/series/") else {
		return Ok(None);
	};
	let after = &query[idx + "/series/".len()..];
	let slug = after.split(['/', '?', '#']).next().unwrap_or("");
	if slug.is_empty() {
		return Ok(None);
	}
	let manga = Chikari.get_manga_update(
		Manga {
			key: slug.to_string(),
			..Default::default()
		},
		true,
		false,
	);
	match manga {
		Ok(manga) if !manga.title.is_empty() => Ok(Some(MangaPageResult {
			entries: vec![manga],
			has_next_page: false,
		})),
		_ => Ok(None),
	}
}

impl Home for Chikari {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut qs = QueryParameters::new();
		qs.push("adult", Some(&settings::adult().to_string()));
		qs.push(
			"content_rating",
			Some(&settings::content_ratings().join(",")),
		);
		for kind in settings::content_types() {
			qs.push("type", Some(&kind));
		}
		// The home payload and every row query are independent, so they all go
		// out together rather than stacking their latencies.
		let mut urls = vec![format!("{API_URL}/home?{qs}")];
		urls.extend(HOME_ROWS.iter().map(|row| row.url()));
		let requests = urls
			.iter()
			.map(|url| api_request(url))
			.collect::<Result<Vec<_>>>()?;
		let mut responses = Request::send_all(requests).into_iter();

		let mut rows = responses
			.next()
			.and_then(|response| response.ok())
			.and_then(|response| response.get_json_owned::<HomeResponse>().ok())
			.map(|data| data.rows)
			.unwrap_or_default();
		let mut components: Vec<HomeComponent> = Vec::new();
		let popular: Vec<Manga> = take_home_row(&mut rows, "popular")
			.into_iter()
			.take(8)
			.map(item_to_manga)
			.collect();
		if !popular.is_empty() {
			components.push(HomeComponent {
				title: Some("Popular".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: popular,
					auto_scroll_interval: Some(5.0),
				},
			});
		}

		for (row, response) in HOME_ROWS.iter().zip(responses) {
			let entries: Vec<Manga> = response
				.ok()
				.and_then(|response| response.get_json_owned::<SeriesListResponse>().ok())
				.map(|data| data.items.into_iter().map(item_to_manga).collect())
				.unwrap_or_default();
			if entries.is_empty() {
				continue;
			}
			let entries = entries.into_iter().map(Into::into).collect();
			components.push(HomeComponent {
				title: Some(row.title.into()),
				subtitle: None,
				// The bookmark row is a ranked countdown; the rest are shelves.
				value: if row.period.is_some() {
					HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries,
						listing: Some(row.listing()),
					}
				} else {
					HomeComponentValue::Scroller {
						entries,
						listing: Some(row.listing()),
					}
				},
			});
		}

		let recently_added: Vec<Link> = take_home_row(&mut rows, "recently-added")
			.into_iter()
			.map(item_to_link)
			.collect();
		if !recently_added.is_empty() {
			components.push(HomeComponent {
				title: Some("Recently Added".into()),
				subtitle: None,
				value: HomeComponentValue::Scroller {
					entries: recently_added,
					listing: Some(Listing {
						id: "added".into(),
						name: "Recently Added".into(),
						..Default::default()
					}),
				},
			});
		}

		let updated: Vec<MangaWithChapter> = take_home_row(&mut rows, "recently-updated")
			.into_iter()
			.filter_map(|item| {
				let latest = item.latest_chapter?;
				let date_uploaded = item.last_chapter_at.as_deref().and_then(parse_iso_date);
				let key = chapter_token(Some(latest));
				let manga = item_to_manga(item);
				Some(MangaWithChapter {
					manga,
					chapter: Chapter {
						key,
						chapter_number: Some(latest),
						date_uploaded,
						..Default::default()
					},
				})
			})
			.collect();
		if !updated.is_empty() {
			components.push(HomeComponent {
				title: Some("Recently Updated".into()),
				subtitle: None,
				value: HomeComponentValue::MangaChapterList {
					page_size: None,
					entries: updated,
					listing: Some(Listing {
						id: "updated".into(),
						name: "Recently Updated".into(),
						..Default::default()
					}),
				},
			});
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for Chikari {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		// Home rows key their listing as `<sort>_<type>`, so peel the type off
		// before resolving the sort.
		let (id, types) = ["manga", "manhwa", "manhua"]
			.into_iter()
			.find_map(|kind| {
				listing
					.id
					.strip_suffix(&format!("_{kind}"))
					.map(|id| (id, vec![kind.to_string()]))
			})
			.unwrap_or((listing.id.as_str(), Vec::new()));
		let sort = match id {
			"popular" => "popular",
			"trending" => "trending",
			"top_rated" => "top_rated",
			"updated" => "updated",
			"added" => "added",
			"most_bookmarked" => "most_bookmarked",
			_ => bail!("Unknown listing"),
		};
		let page = page.max(1);
		let offset = (page - 1) * PAGE_SIZE;
		let data = fetch_series(sort, None, None, &types, &[], offset)?;
		let next_offset = offset + data.items.len() as i32;
		let has_next_page = !data.items.is_empty() && next_offset < data.total;
		let entries = data.items.into_iter().map(item_to_manga).collect();
		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}
}

impl aidoku::ImageRequestProvider for Chikari {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("Origin", DOMAIN))
	}
}

impl DeepLinkHandler for Chikari {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/series/") else {
			return Ok(None);
		};
		let after = &url[idx + "/series/".len()..];
		let slug = after.split(['/', '?', '#']).next().unwrap_or("");
		if slug.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga {
			key: slug.to_string(),
		}))
	}
}

register_source!(
	Chikari,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
