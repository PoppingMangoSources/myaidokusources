#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result, Source, Viewer,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::Deserialize;

mod rsc;

use rsc::*;

const DOMAIN: &str = "https://omanga.to";

#[derive(Deserialize, Default)]
struct CatalogItem {
	#[serde(default)]
	title: String,
	#[serde(default)]
	slug: String,
	#[serde(default)]
	poster: String,
	#[serde(rename = "type")]
	kind: Option<String>,
	genres: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct CatalogResponse {
	#[serde(default)]
	items: Vec<CatalogItem>,
	#[serde(rename = "hasMore", default)]
	has_more: bool,
}

#[derive(Deserialize, Default)]
struct ChapterEntry {
	#[serde(default)]
	number: f32,
	volume: Option<f32>,
	title: Option<String>,
	#[serde(rename = "createdAt")]
	created_at: Option<String>,
	#[serde(rename = "isLocked")]
	is_locked: Option<bool>,
	team: Option<Team>,
}

#[derive(Deserialize, Default)]
struct Team {
	name: Option<String>,
}

#[derive(Deserialize, Default)]
struct SeriesProps {
	#[serde(default)]
	title: String,
	description: Option<String>,
	genres: Option<Vec<String>>,
	tags: Option<Vec<String>>,
	author: Option<String>,
	artist: Option<String>,
	status: Option<String>,
	#[serde(rename = "ageRating")]
	age_rating: Option<String>,
	chapters: Option<Vec<ChapterEntry>>,
}

#[derive(Deserialize, Default)]
struct ReaderChapter {
	pages: Option<Vec<String>>,
	#[serde(rename = "pagesAlt")]
	pages_alt: Option<Vec<String>>,
}

fn fetch_rsc(url: &str) -> Result<String> {
	Request::get(url)?
		.header("RSC", "1")
		.header("Accept", "text/x-component")
		.header("Referer", &format!("{DOMAIN}/"))
		.send()?
		.get_string()
}

fn image_url(path: &str) -> Option<String> {
	let path = path.trim();
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

fn viewer_for(kind: Option<&str>) -> Viewer {
	match kind.unwrap_or("").to_lowercase().as_str() {
		"manhwa" | "manhua" | "webtoon" => Viewer::Webtoon,
		"manga" => Viewer::RightToLeft,
		"comic" => Viewer::LeftToRight,
		_ => Viewer::Unknown,
	}
}

fn status_from(status: Option<&str>) -> MangaStatus {
	match status.unwrap_or("").to_lowercase().as_str() {
		s if s.contains("ongoing") => MangaStatus::Ongoing,
		s if s.contains("completed") => MangaStatus::Completed,
		s if s.contains("hiatus") => MangaStatus::Hiatus,
		s if s.contains("cancel") => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

/// Uses the site's own age rating first, then falls back to genre names.
fn rating_for(age_rating: Option<&str>, genres: &[String]) -> ContentRating {
	let age = age_rating.unwrap_or("");
	if age.starts_with("18") || age.starts_with("21") {
		return ContentRating::NSFW;
	}
	let lowered: Vec<String> = genres.iter().map(|g| g.to_lowercase()).collect();
	if lowered
		.iter()
		.any(|g| ["hentai", "adult", "smut", "lolicon", "shotacon"].contains(&g.as_str()))
	{
		ContentRating::NSFW
	} else if lowered
		.iter()
		.any(|g| ["ecchi", "mature", "harem"].contains(&g.as_str()))
		|| age.starts_with("16")
		|| age.starts_with("15")
	{
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

fn item_to_manga(item: CatalogItem) -> Manga {
	let genres = item.genres.unwrap_or_default();
	Manga {
		key: item.slug,
		title: item.title,
		cover: image_url(&item.poster),
		content_rating: rating_for(None, &genres),
		viewer: viewer_for(item.kind.as_deref()),
		tags: (!genres.is_empty()).then_some(genres),
		..Default::default()
	}
}

fn item_to_link(item: CatalogItem) -> Link {
	let manga = item_to_manga(item);
	Link {
		title: manga.title.clone(),
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn fetch_catalog(query: &str) -> Result<CatalogResponse> {
	Request::get(format!("{DOMAIN}/api/catalog?{query}"))?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Accept", "application/json, text/plain, */*")
		.send()?
		.get_json_owned()
}

fn catalog_page_query(query: &str, page: i32) -> Result<MangaPageResult> {
	let separator = if query.is_empty() { "" } else { "&" };
	let data = fetch_catalog(&format!("{query}{separator}page={}", page.max(1)))?;
	Ok(MangaPageResult {
		has_next_page: data.has_more,
		entries: data.items.into_iter().map(item_to_manga).collect(),
	})
}

fn listing(id: &str, name: &str) -> Option<Listing> {
	Some(Listing {
		id: id.into(),
		name: name.into(),
		..Default::default()
	})
}

fn push_scroller(
	components: &mut Vec<HomeComponent>,
	title: &str,
	id: &str,
	query: &str,
	ranked: bool,
) {
	let Ok(data) = fetch_catalog(&format!("{query}&page=1")) else {
		return;
	};
	let entries: Vec<Link> = data.items.into_iter().map(item_to_link).collect();
	if entries.is_empty() {
		return;
	}
	components.push(HomeComponent {
		title: Some(title.into()),
		subtitle: None,
		value: if ranked {
			HomeComponentValue::MangaList {
				ranking: true,
				page_size: Some(10),
				entries,
				listing: listing(id, title),
			}
		} else {
			HomeComponentValue::Scroller {
				entries,
				listing: listing(id, title),
			}
		},
	});
}

fn sort_id(index: i32) -> &'static str {
	match index {
		1 => "updated_at",
		2 => "created_at",
		3 => "rating",
		4 => "votes",
		5 => "likes",
		6 => "chapters",
		7 => "by_views",
		_ => "real_views",
	}
}

struct OManga;

impl Source for OManga {
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

		let mut qs = QueryParameters::new();
		if !query.is_empty() {
			qs.push("q", Some(query));
		}
		let mut sort = "real_views";
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = sort_id(index),
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => {
					let (include_key, exclude_key) = match id.as_str() {
						"genre" => ("genre", Some("excludeGenre")),
						"type" => ("type", Some("excludeType")),
						"status" => ("status", None),
						_ => continue,
					};
					for value in included {
						qs.push(include_key, Some(&value));
					}
					if let Some(exclude_key) = exclude_key {
						for value in excluded {
							qs.push(exclude_key, Some(&value));
						}
					}
				}
				_ => {}
			}
		}
		qs.push("sort", Some(sort));
		qs.push("page", Some(&page.max(1).to_string()));

		let data = fetch_catalog(&qs.to_string())?;
		Ok(MangaPageResult {
			has_next_page: data.has_more,
			entries: data.items.into_iter().map(item_to_manga).collect(),
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = manga.key.clone();
		let payload = fetch_rsc(&format!("{DOMAIN}/manga/{slug}"))?;
		let Some(mut props) = extract_at_marker::<SeriesProps>(&payload, "{\"initialTab\"", 0)
			.filter(|props| !props.title.is_empty())
		else {
			bail!("No series payload found for {slug}");
		};

		if needs_details {
			if let Some(description) = props.description.as_deref()
				&& description.starts_with('$')
			{
				props.description = resolve_flight_ref(&payload, description);
			}

			let mut tags = props.genres.clone().unwrap_or_default();
			tags.extend(props.tags.clone().unwrap_or_default());

			manga.title = props.title.clone();
			manga.description = props
				.description
				.as_deref()
				.map(|d| d.trim().to_string())
				.filter(|d| !d.is_empty());
			manga.authors = props
				.author
				.as_deref()
				.map(str::trim)
				.filter(|a| !a.is_empty())
				.map(|a| vec![a.to_string()]);
			manga.artists = props
				.artist
				.as_deref()
				.map(str::trim)
				.filter(|a| !a.is_empty())
				.map(|a| vec![a.to_string()]);
			manga.status = status_from(props.status.as_deref());
			manga.content_rating = rating_for(props.age_rating.as_deref(), &tags);
			manga.url = Some(format!("{DOMAIN}/manga/{slug}"));
			manga.tags = (!tags.is_empty()).then_some(tags);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = props
				.chapters
				.unwrap_or_default()
				.into_iter()
				.map(|entry| {
					let scanlators = entry
						.team
						.and_then(|team| team.name)
						.map(|name| vec![name])
						.filter(|names| !names[0].is_empty());
					let chapter_key = entry.number.to_string();
					Chapter {
						url: Some(format!("{DOMAIN}/manga/{slug}/chapter/{chapter_key}")),
						key: chapter_key,
						title: entry
							.title
							.as_deref()
							.map(str::trim)
							.filter(|t| !t.is_empty())
							.map(|t| t.to_string()),
						chapter_number: Some(entry.number),
						volume_number: entry.volume.filter(|v| *v > 0.0),
						date_uploaded: parse_flight_date(entry.created_at.as_deref()),
						scanlators,
						locked: entry.is_locked.unwrap_or(false),
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
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let payload = fetch_rsc(&format!(
			"{DOMAIN}/manga/{}/chapter/{}",
			manga.key, chapter.key
		))?;
		let reader = extract_at_marker::<ReaderChapter>(&payload, "\"chapter\":{\"id\":", 10)
			.ok_or_else(|| error!("No reader payload found"))?;

		let pages = reader
			.pages
			.filter(|pages| !pages.is_empty())
			.or(reader.pages_alt)
			.unwrap_or_default();
		if pages.is_empty() {
			bail!("No pages found for chapter {}", chapter.key);
		}
		Ok(pages
			.into_iter()
			.filter_map(|page| image_url(&page))
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

/// Flight dates arrive as `$D2026-07-14T02:23:00.772Z`.
fn parse_flight_date(value: Option<&str>) -> Option<i64> {
	let raw = value?.trim().trim_start_matches("$D");
	if raw.len() < 19 {
		return None;
	}
	parse_date(raw[..19].replace('T', " "), "yyyy-MM-dd HH:mm:ss")
}

impl Home for OManga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok(data) = fetch_catalog("sort=real_views&page=1") {
			let entries: Vec<Manga> = data.items.into_iter().take(10).map(item_to_manga).collect();
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

		push_scroller(
			&mut components,
			"Latest Updates",
			"updated_at",
			"sort=updated_at",
			false,
		);

		components.push(HomeComponent {
			title: Some("Top Series".into()),
			subtitle: None,
			value: HomeComponentValue::Filters(vec![
				FilterItem {
					title: "From Korea".into(),
					values: Some(vec![FilterValue::MultiSelect {
						id: "type".into(),
						included: vec!["Manhwa".into()],
						excluded: Vec::new(),
					}]),
				},
				FilterItem {
					title: "From Japan".into(),
					values: Some(vec![FilterValue::MultiSelect {
						id: "type".into(),
						included: vec!["Manga".into()],
						excluded: Vec::new(),
					}]),
				},
				FilterItem {
					title: "From China".into(),
					values: Some(vec![FilterValue::MultiSelect {
						id: "type".into(),
						included: vec!["Manhua".into()],
						excluded: Vec::new(),
					}]),
				},
			]),
		});

		for (title, id, query, ranked) in [
			("New Season", "new_season", "sort=created_at", false),
			("Most Liked", "most_liked", "sort=likes", false),
			(
				"Best Ongoings",
				"best_ongoing",
				"sort=rating&status=Ongoing",
				false,
			),
			("In the Trend", "trend", "sort=by_views", false),
			("Popular Today", "popular_today", "sort=votes", true),
		] {
			push_scroller(&mut components, title, id, query, ranked);
		}

		let genres = [
			"Action",
			"Adventure",
			"Comedy",
			"Drama",
			"Fantasy",
			"Historical",
			"Horror",
			"Josei",
			"Martial Arts",
			"Mystery",
			"Psychological",
			"Romance",
			"School Life",
			"Sci-Fi",
			"Seinen",
			"Shoujo",
			"Shounen",
			"Slice of Life",
			"Sports",
			"Supernatural",
			"Tragedy",
		];
		components.push(HomeComponent {
			title: Some("Genres".into()),
			subtitle: None,
			value: HomeComponentValue::Filters(
				genres
					.into_iter()
					.map(|genre| FilterItem {
						title: genre.into(),
						values: Some(vec![FilterValue::MultiSelect {
							id: "genre".into(),
							included: vec![genre.into()],
							excluded: Vec::new(),
						}]),
					})
					.collect(),
			),
		});

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for OManga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let query = match listing.id.as_str() {
			"real_views" => "sort=real_views",
			"updated_at" => "sort=updated_at",
			"created_at" | "new_season" => "sort=created_at",
			"rating" => "sort=rating",
			"most_liked" => "sort=likes",
			"best_ongoing" => "sort=rating&status=Ongoing",
			"trend" => "sort=by_views",
			"popular_today" => "sort=votes",
			_ => bail!("Unknown listing"),
		};
		catalog_page_query(query, page)
	}
}

impl aidoku::ImageRequestProvider for OManga {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", &format!("{DOMAIN}/")))
	}
}

impl DeepLinkHandler for OManga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/manga/") else {
			return Ok(None);
		};
		let mut segments = url[idx + "/manga/".len()..].split('/');
		let Some(slug) = segments
			.next()
			.map(|s| s.split(['?', '#']).next().unwrap_or(s))
			.filter(|s| !s.is_empty())
		else {
			return Ok(None);
		};
		if segments.next() == Some("chapter")
			&& let Some(id) = segments.next().filter(|s| !s.is_empty())
		{
			return Ok(Some(DeepLinkResult::Chapter {
				manga_key: slug.to_string(),
				key: id.split(['?', '#']).next().unwrap_or(id).to_string(),
			}));
		}
		Ok(Some(DeepLinkResult::Manga {
			key: slug.to_string(),
		}))
	}
}

register_source!(
	OManga,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
