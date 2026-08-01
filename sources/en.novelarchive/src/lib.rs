#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, MangaWithChapter, Page, PageContent, Result, Source,
	Viewer,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::defaults::defaults_get,
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::{Deserialize, de::DeserializeOwned};

const DOMAIN: &str = "https://novelarchive.cc";
const API_URL: &str = "https://novelarchive.cc/api";
const PAGE_SIZE: i32 = 24;
const HIDE_ADULT_KEY: &str = "hideAdult";

fn hide_adult() -> bool {
	defaults_get::<bool>(HIDE_ADULT_KEY).unwrap_or(false)
}

const ADULT_GENRES: &[&str] = &["adult", "smut", "erotica", "hentai", "explicit", "nsfw"];
const MATURE_GENRES: &[&str] = &["mature", "ecchi"];

#[derive(Deserialize, Default)]
struct Novel {
	id: serde_json::Value,
	#[serde(default)]
	title: String,
	author: Option<String>,
	genres: Option<String>,
	cover_url: Option<String>,
	image_url: Option<String>,
	novel_image: Option<String>,
	description: Option<String>,
	release_status: Option<String>,
	ongoing: Option<String>,
	updated_at: Option<String>,
	chapter_names: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct NovelListResponse {
	#[serde(default)]
	novels: Vec<Novel>,
	pagination: Option<Pagination>,
}

#[derive(Deserialize)]
struct Pagination {
	#[serde(default)]
	has_next: bool,
}

#[derive(Deserialize)]
struct NovelDetailResponse {
	novel: Novel,
}

#[derive(Deserialize)]
struct ChapterContent {
	content: Option<String>,
}

#[derive(Deserialize)]
struct ChapterDetailResponse {
	chapter: Option<ChapterContent>,
	content: Option<String>,
}

fn api_get<T: DeserializeOwned>(url: &str) -> Result<T> {
	Request::get(url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Origin", DOMAIN)
		.header("Accept", "application/json, text/plain, */*")
		.send()?
		.get_json_owned()
}

fn id_string(value: &serde_json::Value) -> String {
	match value {
		serde_json::Value::String(s) => s.clone(),
		serde_json::Value::Number(n) => n.to_string(),
		_ => String::new(),
	}
}

fn cover_url(novel: &Novel) -> Option<String> {
	let path = novel
		.cover_url
		.as_deref()
		.or(novel.image_url.as_deref())
		.or(novel.novel_image.as_deref())?;
	if path.is_empty() {
		return None;
	}
	Some(if path.starts_with("http") {
		path.to_string()
	} else if path.starts_with('/') {
		format!("{DOMAIN}{path}")
	} else {
		format!("{DOMAIN}/{path}")
	})
}

fn genres_of(novel: &Novel) -> Vec<String> {
	novel
		.genres
		.as_deref()
		.unwrap_or("")
		.split(',')
		.map(|g| g.trim().to_string())
		.filter(|g| !g.is_empty())
		.collect()
}

fn content_rating_for(genres: &[String]) -> ContentRating {
	let lowered: Vec<String> = genres.iter().map(|g| g.trim().to_lowercase()).collect();
	if lowered.iter().any(|g| ADULT_GENRES.contains(&g.as_str())) {
		ContentRating::NSFW
	} else if lowered.iter().any(|g| MATURE_GENRES.contains(&g.as_str())) {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

fn status_of(novel: &Novel) -> MangaStatus {
	let text = novel
		.release_status
		.as_deref()
		.or(novel.ongoing.as_deref())
		.unwrap_or("")
		.to_lowercase();
	if text.contains("complet") || text.contains("finish") {
		MangaStatus::Completed
	} else if text.contains("ongoing") || text.contains("publishing") {
		MangaStatus::Ongoing
	} else if text.contains("hiatus") {
		MangaStatus::Hiatus
	} else if text.contains("cancel") || text.contains("drop") {
		MangaStatus::Cancelled
	} else {
		MangaStatus::Unknown
	}
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
	out.replace("&amp;", "&")
		.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&#39;", "'")
		.replace("&nbsp;", " ")
		.trim()
		.to_string()
}

fn parse_iso(value: Option<&str>) -> Option<i64> {
	let text = value?.trim();
	if text.len() < 19 {
		return None;
	}
	let normalized = text[..19].replace('T', " ");
	parse_date(&normalized, "yyyy-MM-dd HH:mm:ss")
}

fn novel_to_manga(novel: Novel) -> Manga {
	let genres = genres_of(&novel);
	let key = id_string(&novel.id);
	Manga {
		title: novel.title.trim().to_string(),
		cover: cover_url(&novel),
		description: novel
			.description
			.as_deref()
			.map(strip_html)
			.filter(|d| !d.is_empty()),
		authors: novel
			.author
			.as_deref()
			.map(str::trim)
			.filter(|a| !a.is_empty())
			.map(|a| vec![a.to_string()]),
		status: status_of(&novel),
		content_rating: content_rating_for(&genres),
		viewer: Viewer::Vertical,
		url: Some(format!("{DOMAIN}/novel?id={key}")),
		tags: (!genres.is_empty()).then_some(genres),
		key,
		..Default::default()
	}
}

fn novel_to_link(novel: Novel) -> Link {
	let manga = novel_to_manga(novel);
	Link {
		title: manga.title.clone(),
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

/// Splits a printed chapter name into its number and remaining title.
fn clean_chapter_name(name: &str) -> (Option<f32>, String) {
	let trimmed = name.trim();
	let lower = trimmed.to_lowercase();
	for kw in [
		"chapter", "chap.", "chap", "ch.", "ch", "episode", "ep.", "ep",
	] {
		if let Some(rest) = lower.strip_prefix(kw) {
			let offset = trimmed.len() - rest.len();
			let after = &trimmed[offset..];
			let after = after.trim_start();
			let digits: String = after
				.chars()
				.take_while(|c| c.is_ascii_digit() || *c == '.')
				.collect();
			let digits = digits.trim_end_matches('.');
			if let Ok(number) = digits.parse::<f32>() {
				let title = after[digits.len()..]
					.trim_start_matches([' ', '-', ':', '.', '–', '—'])
					.trim();
				return (Some(number), title.to_string());
			}
		}
	}
	(None, trimmed.to_string())
}

fn build_list_url(
	page: i32,
	search: Option<&str>,
	sort: Option<&str>,
	status: Option<&str>,
	genre_match: Option<&str>,
	include: &[String],
	exclude: &[String],
) -> String {
	let mut qs = QueryParameters::new();
	qs.push("page", Some(&page.to_string()));
	qs.push("per_page", Some(&PAGE_SIZE.to_string()));
	if let Some(search) = search.filter(|s| !s.is_empty()) {
		qs.push("search", Some(search));
		qs.push("fuzzy", Some("1"));
	}
	if let Some(sort) = sort.filter(|s| !s.is_empty() && *s != "recent") {
		qs.push("sort", Some(sort));
	}
	if let Some(status) = status.filter(|s| !s.is_empty() && *s != "all") {
		qs.push("status", Some(status));
	}
	if genre_match == Some("any") {
		qs.push("genre_match", Some("any"));
	}
	if !include.is_empty() {
		qs.push("genres_include", Some(&include.join(",")));
	}
	if !exclude.is_empty() {
		qs.push("genres_exclude", Some(&exclude.join(",")));
	}
	format!("{API_URL}/novels?{qs}")
}

fn fetch_list(url: &str) -> Result<MangaPageResult> {
	let data: NovelListResponse = api_get(url)?;
	let hide = hide_adult();
	let has_next_page = data
		.pagination
		.map(|p| p.has_next)
		.unwrap_or(data.novels.len() as i32 == PAGE_SIZE);
	Ok(MangaPageResult {
		entries: data
			.novels
			.into_iter()
			.map(novel_to_manga)
			.filter(|manga| !hide || manga.content_rating != ContentRating::NSFW)
			.collect(),
		has_next_page,
	})
}

struct NovelArchive;

impl Source for NovelArchive {
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
		let page = page.max(1);

		let mut sort = "recent";
		let mut status = "all";
		let mut genre_match = "all";
		let mut include: Vec<String> = Vec::new();
		let mut exclude: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					sort = match index {
						1 => "popular",
						2 => "rating",
						3 => "chapters",
						_ => "recent",
					}
				}
				FilterValue::Select { id, value } if id == "status" => {
					status = match value.as_str() {
						"ongoing" => "ongoing",
						"completed" => "completed",
						"hiatus" => "hiatus",
						_ => "all",
					}
				}
				FilterValue::Select { id, value } if id == "genre_match" => {
					genre_match = if value == "any" { "any" } else { "all" }
				}
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} if id == "genres" => {
					include = included;
					exclude = excluded;
				}
				_ => {}
			}
		}

		fetch_list(&build_list_url(
			page,
			Some(query),
			Some(sort),
			Some(status),
			Some(genre_match),
			&include,
			&exclude,
		))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let novel_id = manga.key.clone();
		let data: NovelDetailResponse = api_get(&format!("{API_URL}/novels/{novel_id}"))?;
		let novel = data.novel;

		let chapter_names = novel.chapter_names.clone().unwrap_or_default();

		if needs_details {
			let mut details = novel_to_manga(novel);
			details.key = novel_id.clone();
			manga.copy_from(details);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let total = chapter_names.len();
			let mut chapters: Vec<Chapter> = chapter_names
				.into_iter()
				.enumerate()
				.map(|(index, raw)| {
					let (number, title) = clean_chapter_name(&raw);
					let chapter_number = number.unwrap_or((index + 1) as f32);
					Chapter {
						// Reader ids are 1-based positions, not the printed number.
						key: (index + 1).to_string(),
						title: (!title.is_empty()).then_some(title),
						chapter_number: Some(chapter_number),
						language: Some("en".into()),
						..Default::default()
					}
				})
				.collect();
			if total > 0 {
				chapters.reverse();
			}
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let data: ChapterDetailResponse = api_get(&format!(
			"{API_URL}/novels/{}/chapters/{}",
			manga.key, chapter.key
		))?;
		let content = data
			.chapter
			.and_then(|c| c.content)
			.or(data.content)
			.unwrap_or_default();
		let text = strip_html(&content);
		if text.is_empty() {
			bail!("No readable content found");
		}
		Ok(vec![Page {
			content: PageContent::text(text),
			..Default::default()
		}])
	}
}

impl Home for NovelArchive {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok(data) = api_get::<NovelListResponse>(&build_list_url(
			1,
			None,
			Some("popular"),
			None,
			None,
			&[],
			&[],
		)) {
			let entries: Vec<Manga> = data
				.novels
				.into_iter()
				.take(10)
				.map(novel_to_manga)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Popular".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(5.0),
					},
				});
			}
		}

		if let Ok(data) = api_get::<NovelListResponse>(&format!(
			"{API_URL}/novels/recently-updated?limit={PAGE_SIZE}"
		)) {
			let entries: Vec<MangaWithChapter> = data
				.novels
				.into_iter()
				.filter_map(|novel| {
					let names = novel.chapter_names.clone().unwrap_or_default();
					let total = names.len();
					let last = names.last()?;
					let (number, title) = clean_chapter_name(last);
					let date_uploaded = parse_iso(novel.updated_at.as_deref());
					let manga = novel_to_manga(novel);
					Some(MangaWithChapter {
						manga,
						chapter: Chapter {
							key: total.to_string(),
							title: (!title.is_empty()).then_some(title),
							chapter_number: Some(number.unwrap_or(total as f32)),
							date_uploaded,
							..Default::default()
						},
					})
				})
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Latest Updates".into()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: None,
						entries,
						listing: Some(Listing {
							id: "recent".into(),
							name: "Recently Updated".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		if let Ok(data) = api_get::<NovelListResponse>(&format!("{API_URL}/novels/editors-choice"))
		{
			let entries: Vec<Manga> = data.novels.into_iter().map(novel_to_manga).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Editor's Choice".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					},
				});
			}
		}

		for (title, sort) in [("Top Rated", "rating"), ("Most Chapters", "chapters")] {
			if let Ok(data) = api_get::<NovelListResponse>(&build_list_url(
				1,
				None,
				Some(sort),
				None,
				None,
				&[],
				&[],
			)) {
				let entries: Vec<Link> = data.novels.into_iter().map(novel_to_link).collect();
				if !entries.is_empty() {
					components.push(HomeComponent {
						title: Some(title.into()),
						subtitle: None,
						value: if sort == "rating" {
							HomeComponentValue::MangaList {
								ranking: true,
								page_size: Some(10),
								entries,
								listing: Some(Listing {
									id: sort.into(),
									name: title.into(),
									..Default::default()
								}),
							}
						} else {
							HomeComponentValue::Scroller {
								entries,
								listing: Some(Listing {
									id: sort.into(),
									name: title.into(),
									..Default::default()
								}),
							}
						},
					});
				}
			}
		}

		let genres = [
			"Action",
			"Adventure",
			"Comedy",
			"Drama",
			"Fantasy",
			"Historical",
			"Horror",
			"Mystery",
			"Romance",
			"Sci-fi",
			"Slice of Life",
			"Supernatural",
			"Tragedy",
			"Wuxia",
			"Xianxia",
		]
		.into_iter()
		.map(|genre| FilterItem {
			title: genre.into(),
			values: Some(vec![FilterValue::MultiSelect {
				id: "genres".into(),
				included: vec![genre.into()],
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
}

impl ListingProvider for NovelArchive {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let sort = match listing.id.as_str() {
			"popular" => "popular",
			"recent" => "recent",
			"rating" => "rating",
			"chapters" => "chapters",
			_ => bail!("Unknown listing"),
		};
		fetch_list(&build_list_url(
			page.max(1),
			None,
			Some(sort),
			None,
			None,
			&[],
			&[],
		))
	}
}

impl DeepLinkHandler for NovelArchive {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// ex: https://novelarchive.cc/novel?id=1234
		let Some(idx) = url.find("id=") else {
			return Ok(None);
		};
		let id: String = url[idx + 3..]
			.chars()
			.take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
			.collect();
		if id.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga { key: id }))
	}
}

register_source!(NovelArchive, Home, ListingProvider, DeepLinkHandler);
