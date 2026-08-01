#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::defaults::defaults_get,
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::de::DeserializeOwned;

mod models;

use models::*;

const BASE_URL_KEY: &str = "baseUrl";
const API_URL_KEY: &str = "apiUrl";
const CONTENT_PREFERENCE_KEY: &str = "contentPreference";
const HIDDEN_GENRES_KEY: &str = "hiddenGenres";

fn override_url(key: &str, fallback: &str) -> String {
	defaults_get::<String>(key)
		.map(|url| url.trim().trim_end_matches('/').to_string())
		.filter(|url| url.starts_with("http"))
		.unwrap_or_else(|| fallback.into())
}

fn site_url() -> String {
	override_url(BASE_URL_KEY, DOMAIN)
}

fn api_url() -> String {
	override_url(API_URL_KEY, API_URL)
}

fn safe_only() -> bool {
	defaults_get::<String>(CONTENT_PREFERENCE_KEY).as_deref() != Some("all")
}

fn hidden_genres() -> Vec<i64> {
	defaults_get::<Vec<String>>(HIDDEN_GENRES_KEY)
		.unwrap_or_default()
		.into_iter()
		.filter_map(|id| id.parse().ok())
		.collect()
}

/// Applies the reader's content preference and hidden-genre list.
fn is_visible(series: &SeriesDto) -> bool {
	let tags = series.tags.as_deref().unwrap_or(&[]);
	let hidden = hidden_genres();
	if tags.iter().any(|id| hidden.contains(id)) {
		return false;
	}
	if safe_only() && derive_content_rating(series.content_rating, tags) != ContentRating::Safe {
		return false;
	}
	true
}

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

fn get<T: DeserializeOwned>(url: &str) -> Result<T> {
	Request::get(url)?
		.header("Referer", &format!("{}/", site_url()))
		.header("Origin", &site_url())
		.header("User-Agent", USER_AGENT)
		.header("Accept", "application/json, text/plain, */*")
		.send()?
		.get_json_owned()
}

fn cover_url(cover: Option<&str>) -> Option<String> {
	match cover {
		Some(c) if !c.is_empty() => {
			if c.starts_with("http") {
				Some(c.to_string())
			} else {
				Some(format!("{CDN_URL}/covers/{c}"))
			}
		}
		_ => None,
	}
}

fn number_value(value: Option<&serde_json::Value>) -> f32 {
	match value {
		Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
		Some(serde_json::Value::String(s)) => s.trim().parse::<f32>().unwrap_or(0.0),
		_ => 0.0,
	}
}

fn parse_datetime(value: &str) -> Option<i64> {
	let bytes = value.as_bytes();
	let mut normalized: Vec<u8> = bytes.iter().take(19).copied().collect();
	if normalized.len() < 19 {
		return None;
	}
	if normalized[10] == b'T' {
		normalized[10] = b' ';
	}
	let normalized = String::from_utf8(normalized).ok()?;
	parse_date(&normalized, "yyyy-MM-dd HH:mm:ss")
}

fn strip_html(html: &str) -> String {
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
	out.trim().replace("&amp;", "&").replace("&#39;", "'")
}

fn scanlation_team(group: Option<&GroupDto>, collab: Option<&[GroupDto]>) -> Option<String> {
	let mut names: Vec<String> = Vec::new();
	if let Some(title) = group.and_then(|g| g.title.as_deref()) {
		let trimmed = title.trim();
		if !trimmed.is_empty() {
			names.push(trimmed.to_string());
		}
	}
	for g in collab.unwrap_or(&[]) {
		if let Some(title) = g.title.as_deref() {
			let trimmed = title.trim();
			if !trimmed.is_empty() && !names.iter().any(|n| n == trimmed) {
				names.push(trimmed.to_string());
			}
		}
	}
	if names.is_empty() {
		None
	} else {
		Some(names.join(", "))
	}
}

fn is_number_only(text: &str) -> bool {
	let lowered = text.trim().to_lowercase();
	let rest = [
		"chapter", "chap.", "chap", "ch.", "ch", "episode", "ep.", "ep",
	]
	.iter()
	.find_map(|kw| lowered.strip_prefix(kw))
	.unwrap_or(&lowered)
	.trim();
	!rest.is_empty()
		&& rest
			.chars()
			.all(|c| c.is_ascii_digit() || c == '.' || c == ' ')
}

fn parse_chapter_title(raw: Option<&str>) -> (Option<String>, Option<String>) {
	let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
		return (None, None);
	};
	let (name_part, group_part) = match raw.split_once('•') {
		Some((name, group)) => (name.trim(), group.trim()),
		None => (raw, ""),
	};
	let title = if !name_part.is_empty() && !is_number_only(name_part) {
		Some(name_part.to_string())
	} else {
		None
	};
	let group = (!group_part.is_empty()).then(|| group_part.to_string());
	(title, group)
}

fn series_to_manga(series: SeriesDto) -> Manga {
	let tags = series.tags.clone().unwrap_or_default();
	let content_rating = derive_content_rating(series.content_rating, &tags);
	let viewer = viewer_for_type(series.kind);
	let cover = cover_url(series.cover.as_deref());
	Manga {
		key: series.id.to_string(),
		title: series.title,
		cover,
		status: map_status(series.status),
		content_rating,
		viewer,
		..Default::default()
	}
}

fn series_to_link(series: SeriesDto) -> Link {
	let manga = series_to_manga(series);
	Link {
		title: manga.title.clone(),
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn series_to_manga_chapter(mut series: SeriesDto) -> Option<MangaWithChapter> {
	let latest = series.chapters.take()?.into_iter().next()?;
	let id = latest.id?;
	let chapter_number = number_value(latest.number.as_ref());
	let date_uploaded = latest
		.updated_at
		.as_deref()
		.or(latest.created_at.as_deref())
		.and_then(parse_datetime);
	let key = format!("{}:{}:{}", series.id, id, latest.group_id.unwrap_or(0));
	let manga = series_to_manga(series);
	Some(MangaWithChapter {
		manga,
		chapter: Chapter {
			key,
			chapter_number: Some(chapter_number),
			date_uploaded,
			..Default::default()
		},
	})
}

fn fetch_series_list(query: &str) -> Result<(Vec<SeriesDto>, bool)> {
	let response: ResponseDto<Vec<SeriesDto>> = get(&format!("{}/series?{query}", api_url()))?;
	let data = response.data.unwrap_or_default();
	let has_more =
		response.meta.map(|m| m.has_more).unwrap_or(false) || data.len() as i32 == SERIES_PAGE_SIZE;
	Ok((data.into_iter().filter(is_visible).collect(), has_more))
}

struct ScansGG;

impl Source for ScansGG {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let term = query.unwrap_or_default();
		let term = term.trim();

		if let Some(result) = resolve_direct_query(term)? {
			return Ok(result);
		}

		let mut types: Vec<String> = Vec::new();
		let mut statuses: Vec<String> = Vec::new();
		let mut genres: Vec<String> = Vec::new();
		let mut popular = String::new();
		for filter in filters {
			match filter {
				FilterValue::MultiSelect { id, included, .. } if id == "type" => types = included,
				FilterValue::MultiSelect { id, included, .. } if id == "status" => {
					statuses = included
				}
				FilterValue::MultiSelect { id, included, .. } if id == "genres" => {
					genres = included
				}
				FilterValue::Select { id, value } if id == "popular" => popular = value,
				_ => {}
			}
		}

		if !popular.is_empty() && term.is_empty() {
			let mut qs = QueryParameters::new();
			qs.push("popular", Some(&popular));
			qs.push("limit", Some(&POPULAR_FETCH_SIZE.to_string()));
			qs.push("chapters", Some("true"));
			qs.push("group_details", Some("true"));
			let (data, _) = fetch_series_list(&qs.to_string())?;
			return Ok(MangaPageResult {
				entries: data.into_iter().map(series_to_manga).collect(),
				has_next_page: false,
			});
		}

		let page = page.max(1);
		let offset = (page - 1) * SERIES_PAGE_SIZE;
		let mut qs = QueryParameters::new();
		qs.push("limit", Some(&SERIES_PAGE_SIZE.to_string()));
		qs.push("offset", Some(&offset.to_string()));
		if !term.is_empty() {
			qs.push("q", Some(term));
		}
		if !types.is_empty() {
			qs.push("q_type", Some(&format!("[{}]", types.join(","))));
		}
		if !statuses.is_empty() {
			qs.push("q_status", Some(&format!("[{}]", statuses.join(","))));
		}
		if genres.len() == 1 {
			qs.push("q_tags", Some(&format!("[{}]", genres[0])));
		}

		let (data, has_next_page) = fetch_series_list(&qs.to_string())?;
		Ok(MangaPageResult {
			entries: data.into_iter().map(series_to_manga).collect(),
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let series_id = manga.key.clone();

		if needs_details {
			let mut qs = QueryParameters::new();
			qs.push("id", Some(&series_id));
			qs.push("trackers", Some("true"));
			qs.push("sources", Some("true"));
			let response: ResponseDto<SeriesDto> = get(&format!("{}/series?{qs}", api_url()))?;
			let series = response.data.ok_or_else(|| error!("No series data"))?;

			let tags_ids = series.tags.clone().unwrap_or_default();
			manga.title = series.title;
			manga.cover = cover_url(series.cover.as_deref());
			if let Some(summary) = series.summary.as_deref() {
				let synopsis = strip_html(summary);
				if !synopsis.is_empty() {
					manga.description = Some(synopsis);
				}
			}
			manga.status = map_status(series.status);
			manga.viewer = viewer_for_type(series.kind);
			manga.content_rating = derive_content_rating(series.content_rating, &tags_ids);
			manga.url = Some(format!("{}/series/{series_id}", site_url()));

			let authors: Vec<String> = series
				.author
				.unwrap_or_default()
				.into_iter()
				.map(|a| a.trim().to_string())
				.filter(|a| !a.is_empty())
				.collect();
			let artists: Vec<String> = series
				.artist
				.unwrap_or_default()
				.into_iter()
				.map(|a| a.trim().to_string())
				.filter(|a| !a.is_empty())
				.collect();
			if !authors.is_empty() {
				manga.authors = Some(authors);
			}
			if !artists.is_empty() {
				manga.artists = Some(artists);
			}

			let mut tags: Vec<String> = Vec::new();
			if let Some(name) = type_name(series.kind) {
				tags.push(name.into());
			}
			for id in &tags_ids {
				if let Some(name) = tag_name(*id) {
					tags.push(name.into());
				}
			}
			for theme in series.themes.unwrap_or_default() {
				let trimmed = theme.trim();
				if !trimmed.is_empty() {
					tags.push(trimmed.to_string());
				}
			}
			if !tags.is_empty() {
				manga.tags = Some(tags);
			}

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			manga.chapters = Some(fetch_chapters(&series_id)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let mut parts = chapter.key.split(':');
		let series_id = parts.next().unwrap_or("");
		let chapter_id = parts.next().unwrap_or("");
		let group_id = parts.next().unwrap_or("0");

		let mut qs = QueryParameters::new();
		qs.push("series_id", Some(series_id));
		qs.push("chapter_id", Some(chapter_id));
		if group_id != "0" {
			qs.push("group_id", Some(group_id));
		}

		let response: ResponseDto<PageListDto> =
			get(&format!("{}/chapter-navigation?{qs}", api_url()))?;
		let chapter_data = response
			.data
			.and_then(|d| d.chapter)
			.ok_or_else(|| error!("No page data"))?;
		let page_chapter_id = chapter_data
			.id
			.map(|id| id.to_string())
			.unwrap_or_else(|| chapter_id.to_string());

		let mut pages = chapter_data.pages.unwrap_or_default();
		pages.sort_by_key(|p| p.position);
		if pages.is_empty() {
			bail!("No pages found");
		}
		Ok(pages
			.into_iter()
			.map(|p| Page {
				content: PageContent::url(format!("{CDN_URL}/pages/{page_chapter_id}/{}", p.path)),
				..Default::default()
			})
			.collect())
	}
}

fn fetch_chapters(series_id: &str) -> Result<Vec<Chapter>> {
	let mut all: Vec<ChapterDto> = Vec::new();
	let mut page = 1;
	loop {
		let mut qs = QueryParameters::new();
		qs.push("series_id", Some(series_id));
		qs.push("limit", Some(&CHAPTER_PAGE_SIZE.to_string()));
		qs.push("page", Some(&page.to_string()));
		qs.push("group_details", Some("true"));
		let response: ResponseDto<Vec<ChapterDto>> = get(&format!("{}/chapters?{qs}", api_url()))?;
		let batch = response.data.unwrap_or_default();
		let has_more = response.meta.map(|m| m.has_more).unwrap_or(false) && !batch.is_empty();
		all.extend(batch);
		if !has_more || page >= 200 {
			break;
		}
		page += 1;
	}

	Ok(all
		.into_iter()
		.map(|ch| {
			let chapter_number = number_value(ch.number.as_ref());
			let (title, group_from_title) = parse_chapter_title(ch.title.as_deref());
			let scanlator = scanlation_team(ch.group.as_ref(), ch.collab_groups.as_deref())
				.or(group_from_title);
			let key = format!("{series_id}:{}:{}", ch.id, ch.group_id.unwrap_or(0));
			Chapter {
				url: Some(format!("{}/series/{series_id}/{}", site_url(), ch.id)),
				key,
				title,
				chapter_number: Some(chapter_number),
				scanlators: scanlator.map(|s| vec![s]),
				date_uploaded: ch.created_at.as_deref().and_then(parse_datetime),
				language: Some("en".into()),
				..Default::default()
			}
		})
		.collect())
}

fn resolve_direct_query(query: &str) -> Result<Option<MangaPageResult>> {
	let id = if let Some(rest) = query.to_lowercase().strip_prefix("id:") {
		let value = rest.trim();
		value
			.chars()
			.all(|c| c.is_ascii_digit())
			.then(|| value.to_string())
			.filter(|v| !v.is_empty())
	} else if let Some(idx) = query.find("/series/") {
		let after = &query[idx + "/series/".len()..];
		let segment = after.split(['/', '?', '#']).next().unwrap_or("");
		let numeric: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
		(!numeric.is_empty()).then_some(numeric)
	} else {
		None
	};

	let Some(id) = id else {
		return Ok(None);
	};

	let manga = ScansGG.get_manga_update(
		Manga {
			key: id,
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

impl Home for ScansGG {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		// Top Manga (monthly popular)
		let mut qs = QueryParameters::new();
		qs.push("popular", Some("monthly"));
		qs.push("limit", Some(&POPULAR_FETCH_SIZE.to_string()));
		qs.push("chapters", Some("true"));
		qs.push("group_details", Some("true"));
		if let Ok((data, _)) = fetch_series_list(&qs.to_string()) {
			let entries: Vec<Manga> = data
				.into_iter()
				.filter(|s| s.cover.is_some())
				.take(15)
				.map(series_to_manga)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Top Manga".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(5.0),
					},
				});
			}
		}

		// Popular over shorter windows.
		for (title, listing_id, range) in [
			("Popular Today", "popular_daily", "daily"),
			("Popular This Week", "popular_weekly", "weekly"),
		] {
			let mut qs = QueryParameters::new();
			qs.push("popular", Some(range));
			qs.push("limit", Some(&POPULAR_FETCH_SIZE.to_string()));
			qs.push("chapters", Some("true"));
			qs.push("group_details", Some("true"));
			if let Ok((data, _)) = fetch_series_list(&qs.to_string()) {
				let entries: Vec<Link> = data
					.into_iter()
					.filter(|s| s.cover.is_some())
					.map(series_to_link)
					.collect();
				if !entries.is_empty() {
					components.push(HomeComponent {
						title: Some(title.into()),
						subtitle: None,
						value: HomeComponentValue::Scroller {
							entries,
							listing: Some(Listing {
								id: listing_id.into(),
								name: title.into(),
								..Default::default()
							}),
						},
					});
				}
			}
		}

		// Ranked monthly chart.
		let mut qs = QueryParameters::new();
		qs.push("popular", Some("monthly"));
		qs.push("limit", Some(&POPULAR_FETCH_SIZE.to_string()));
		qs.push("chapters", Some("true"));
		qs.push("group_details", Some("true"));
		if let Ok((data, _)) = fetch_series_list(&qs.to_string()) {
			let entries: Vec<Link> = data
				.into_iter()
				.filter(|s| s.cover.is_some())
				.map(series_to_link)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Popular This Month".into()),
					subtitle: None,
					value: HomeComponentValue::MangaList {
						ranking: true,
						page_size: Some(10),
						entries,
						listing: Some(Listing {
							id: "popular_monthly".into(),
							name: "Popular This Month".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		// Latest Updates
		let mut qs = QueryParameters::new();
		qs.push("page", Some("1"));
		qs.push("limit", Some(&LATEST_PAGE_SIZE.to_string()));
		qs.push("chapters", Some("true"));
		qs.push("series_details", Some("true"));
		qs.push("group_details", Some("true"));
		qs.push("sort", Some("date"));
		if let Ok(response) =
			get::<ResponseDto<Vec<SeriesDto>>>(&format!("{}/chapters?{qs}", api_url()))
		{
			let entries: Vec<MangaWithChapter> = response
				.data
				.unwrap_or_default()
				.into_iter()
				.filter(is_visible)
				.filter_map(series_to_manga_chapter)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Latest Updates".into()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: None,
						entries,
						listing: Some(Listing {
							id: "latest".into(),
							name: "Latest Updates".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		// All Series
		let mut qs = QueryParameters::new();
		qs.push("limit", Some(&SERIES_PAGE_SIZE.to_string()));
		qs.push("offset", Some("0"));
		if let Ok((data, _)) = fetch_series_list(&qs.to_string()) {
			let entries: Vec<Link> = data
				.into_iter()
				.filter(|s| s.cover.is_some())
				.map(series_to_link)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("All Series".into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: Some(Listing {
							id: "all".into(),
							name: "All Series".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		// Genre shortcuts.
		let genre_items: Vec<FilterItem> = (1..=49)
			.filter_map(|id| tag_name(id).map(|name| (id, name)))
			.map(|(id, name)| FilterItem {
				title: name.into(),
				values: Some(vec![FilterValue::MultiSelect {
					id: "genres".into(),
					included: vec![id.to_string()],
					excluded: Vec::new(),
				}]),
			})
			.collect();
		if !genre_items.is_empty() {
			components.push(HomeComponent {
				title: Some("Genres".into()),
				subtitle: None,
				value: HomeComponentValue::Filters(genre_items),
			});
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for ScansGG {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let page = page.max(1);
		match listing.id.as_str() {
			"popular_daily" | "popular_weekly" | "popular_monthly" => {
				let range = match listing.id.as_str() {
					"popular_daily" => "daily",
					"popular_weekly" => "weekly",
					_ => "monthly",
				};
				let mut qs = QueryParameters::new();
				qs.push("popular", Some(range));
				qs.push("limit", Some(&POPULAR_FETCH_SIZE.to_string()));
				qs.push("chapters", Some("true"));
				qs.push("group_details", Some("true"));
				let (data, _) = fetch_series_list(&qs.to_string())?;
				Ok(MangaPageResult {
					entries: data.into_iter().map(series_to_manga).collect(),
					has_next_page: false,
				})
			}
			"all" => {
				let offset = (page - 1) * SERIES_PAGE_SIZE;
				let mut qs = QueryParameters::new();
				qs.push("limit", Some(&SERIES_PAGE_SIZE.to_string()));
				qs.push("offset", Some(&offset.to_string()));
				let (data, has_next_page) = fetch_series_list(&qs.to_string())?;
				Ok(MangaPageResult {
					entries: data.into_iter().map(series_to_manga).collect(),
					has_next_page,
				})
			}
			"latest" => {
				let mut qs = QueryParameters::new();
				qs.push("page", Some(&page.to_string()));
				qs.push("limit", Some(&LATEST_PAGE_SIZE.to_string()));
				qs.push("chapters", Some("true"));
				qs.push("series_details", Some("true"));
				qs.push("group_details", Some("true"));
				qs.push("sort", Some("date"));
				let response: ResponseDto<Vec<SeriesDto>> =
					get(&format!("{}/chapters?{qs}", api_url()))?;
				let data = response.data.unwrap_or_default();
				let has_next_page = response.meta.map(|m| m.has_more).unwrap_or(false)
					|| data.len() as i32 == LATEST_PAGE_SIZE;
				Ok(MangaPageResult {
					entries: data
						.into_iter()
						.filter(is_visible)
						.map(series_to_manga)
						.collect(),
					has_next_page,
				})
			}
			_ => bail!("Unknown listing"),
		}
	}
}

impl aidoku::ImageRequestProvider for ScansGG {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{}/", site_url()))
			.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for ScansGG {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/series/") else {
			return Ok(None);
		};
		let after = &url[idx + "/series/".len()..];
		let segment = after.split(['/', '?', '#']).next().unwrap_or("");
		let numeric: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
		if numeric.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga { key: numeric }))
	}
}

register_source!(
	ScansGG,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
