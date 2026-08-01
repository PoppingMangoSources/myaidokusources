#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	Viewer,
	alloc::{String, Vec, string::ToString, vec},
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::Deserialize;

mod rsc;

use rsc::*;

const DOMAIN: &str = "https://valirscans.org";

#[derive(Deserialize, Clone, Default)]
struct Genre {
	genre: Option<GenreInner>,
	slug: Option<String>,
	name: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
struct GenreInner {
	slug: Option<String>,
	name: Option<String>,
}

impl Genre {
	fn label(&self) -> Option<String> {
		let inner = self.genre.clone().unwrap_or_default();
		inner
			.name
			.or(self.name.clone())
			.or(inner.slug)
			.or(self.slug.clone())
			.map(|value| value.trim().to_string())
			.filter(|value| !value.is_empty())
	}
}

#[derive(Deserialize, Clone, Default)]
struct TagEntry {
	name: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
struct ChapterItem {
	#[serde(default)]
	id: String,
	#[serde(default)]
	number: f32,
	title: Option<String>,
	#[serde(rename = "isLocked")]
	is_locked: Option<bool>,
	#[serde(rename = "publishedAt")]
	published_at: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
struct Series {
	#[serde(default)]
	slug: String,
	#[serde(rename = "urlSlug")]
	url_slug: Option<String>,
	#[serde(default)]
	title: String,
	#[serde(rename = "type")]
	kind: Option<String>,
	#[serde(rename = "coverImage")]
	cover_image: Option<String>,
	description: Option<String>,
	status: Option<String>,
	#[serde(rename = "isMature")]
	is_mature: Option<bool>,
	author: Option<String>,
	artist: Option<String>,
	genres: Option<Vec<Genre>>,
	tags: Option<Vec<TagEntry>>,
	chapters: Option<Vec<ChapterItem>>,
}

#[derive(Deserialize, Default)]
struct SeriesPage {
	series: Series,
	#[serde(default)]
	chapters: Vec<ChapterItem>,
}

#[derive(Deserialize, Default)]
struct ReaderPage {
	#[serde(rename = "pageNumber", default)]
	page_number: i64,
	#[serde(rename = "imageUrl", default)]
	image_url: String,
}

#[derive(Deserialize, Default)]
struct ChapterData {
	content: Option<String>,
	pages: Option<Vec<ReaderPage>>,
}

fn fetch(url: &str, rsc: bool) -> Result<String> {
	let mut request = Request::get(url)?.header("Referer", &format!("{DOMAIN}/"));
	if rsc {
		request = request.header("RSC", "1");
	}
	request.send()?.get_string()
}

fn image_url(path: Option<&str>) -> Option<String> {
	let path = path?.trim();
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

fn is_novel(series: &Series) -> bool {
	series
		.kind
		.as_deref()
		.unwrap_or("")
		.to_uppercase()
		.contains("NOVEL")
}

fn status_from(status: Option<&str>) -> MangaStatus {
	match status.unwrap_or("").to_lowercase().as_str() {
		s if s.contains("ongoing") => MangaStatus::Ongoing,
		s if s.contains("completed") => MangaStatus::Completed,
		s if s.contains("hiatus") => MangaStatus::Hiatus,
		s if s.contains("cancel") || s.contains("drop") => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

fn parse_iso(value: Option<&str>) -> Option<i64> {
	let raw = value?.trim().trim_start_matches("$D");
	if raw.len() < 19 {
		return None;
	}
	parse_date(raw[..19].replace('T', " "), "yyyy-MM-dd HH:mm:ss")
}

/// Series live at `/series/{comic|novel}/{urlSlug}`, so the key keeps both.
fn key_of(series: &Series) -> String {
	let slug = series
		.url_slug
		.as_deref()
		.filter(|s| !s.is_empty())
		.unwrap_or(&series.slug);
	let kind = if is_novel(series) { "novel" } else { "comic" };
	format!("{kind}/{slug}")
}

fn series_to_manga(series: Series) -> Manga {
	let novel = is_novel(&series);
	let mut tags: Vec<String> = series
		.genres
		.as_ref()
		.map(|genres| genres.iter().filter_map(|g| g.label()).collect())
		.unwrap_or_default();
	tags.extend(
		series
			.tags
			.as_ref()
			.map(|entries| {
				entries
					.iter()
					.filter_map(|t| t.name.as_deref())
					.map(|name| name.trim().to_string())
					.filter(|name| !name.is_empty())
					.collect::<Vec<_>>()
			})
			.unwrap_or_default(),
	);

	Manga {
		key: key_of(&series),
		title: series.title,
		cover: image_url(series.cover_image.as_deref()),
		description: series
			.description
			.as_deref()
			.map(|d| d.trim().to_string())
			.filter(|d| !d.is_empty()),
		authors: series
			.author
			.as_deref()
			.map(str::trim)
			.filter(|a| !a.is_empty())
			.map(|a| vec![a.to_string()]),
		artists: series
			.artist
			.as_deref()
			.map(str::trim)
			.filter(|a| !a.is_empty())
			.map(|a| vec![a.to_string()]),
		status: status_from(series.status.as_deref()),
		content_rating: if series.is_mature.unwrap_or(false) {
			ContentRating::NSFW
		} else {
			ContentRating::Safe
		},
		viewer: if novel {
			Viewer::Vertical
		} else {
			Viewer::Webtoon
		},
		tags: (!tags.is_empty()).then_some(tags),
		..Default::default()
	}
}

fn series_to_link(series: Series) -> Link {
	let manga = series_to_manga(series);
	Link {
		title: manga.title.clone(),
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn browse(page: i32) -> Result<Vec<Series>> {
	let payload = fetch(&format!("{DOMAIN}/series?page={}", page.max(1)), false)?;
	let lists: Vec<Vec<Series>> = extract_all_by_marker(&payload, "\"initialSeries\":", false);
	Ok(lists.into_iter().next().unwrap_or_default())
}

struct ValirScans;

impl Source for ValirScans {
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
		let query = query.trim().to_lowercase();

		let mut kind = String::new();
		for filter in filters {
			if let FilterValue::Select { id, value } = filter
				&& id == "type"
			{
				kind = value;
			}
		}

		let mut series = browse(page)?;
		let has_next_page = !series.is_empty();
		series.retain(|entry| {
			if !query.is_empty() && !entry.title.to_lowercase().contains(&query) {
				return false;
			}
			match kind.as_str() {
				"novel" => is_novel(entry),
				"comic" => !is_novel(entry),
				_ => true,
			}
		});

		Ok(MangaPageResult {
			entries: series.into_iter().map(series_to_manga).collect(),
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let key = manga.key.clone();
		let payload = fetch(&format!("{DOMAIN}/series/{key}"), false)?;
		let Some(page) = extract_all_by_marker::<SeriesPage>(&payload, "{\"series\":", true)
			.into_iter()
			.find(|candidate| !candidate.series.title.is_empty())
		else {
			bail!("No series data found for {key}");
		};

		let novel = is_novel(&page.series);

		if needs_details {
			let mut details = series_to_manga(page.series.clone());
			details.key = key.clone();
			details.url = Some(format!("{DOMAIN}/series/{key}"));
			manga.copy_from(details);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut items = page.chapters;
			if items.is_empty() {
				items = page.series.chapters.unwrap_or_default();
			}
			let mut chapters: Vec<Chapter> = items
				.into_iter()
				.filter(|item| !item.id.is_empty())
				.map(|item| Chapter {
					url: Some(format!("{DOMAIN}/series/{key}/chapter/{}", item.id)),
					key: item.id,
					title: item
						.title
						.as_deref()
						.map(str::trim)
						.filter(|t| !t.is_empty())
						.map(|t| t.to_string()),
					chapter_number: Some(item.number),
					date_uploaded: parse_iso(item.published_at.as_deref()),
					locked: item.is_locked.unwrap_or(false),
					language: Some("en".into()),
					..Default::default()
				})
				.collect();
			chapters.sort_by(|a, b| {
				b.chapter_number
					.partial_cmp(&a.chapter_number)
					.unwrap_or(core::cmp::Ordering::Equal)
			});
			manga.chapters = Some(chapters);
		}

		if novel {
			manga.viewer = Viewer::Vertical;
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let payload = fetch(
			&format!("{DOMAIN}/series/{}/chapter/{}", manga.key, chapter.key),
			true,
		)?;
		let Some(data) = extract_all_by_marker::<ChapterData>(&payload, "\"chapter\":", false)
			.into_iter()
			.find(|candidate| candidate.pages.is_some() || candidate.content.is_some())
		else {
			bail!("No chapter data found for {}", chapter.key);
		};

		let mut pages = data.pages.unwrap_or_default();
		if !pages.is_empty() {
			pages.sort_by_key(|page| page.page_number);
			let images: Vec<Page> = pages
				.into_iter()
				.filter_map(|page| image_url(Some(&page.image_url)))
				.map(|url| Page {
					content: PageContent::url(url),
					..Default::default()
				})
				.collect();
			if !images.is_empty() {
				return Ok(images);
			}
		}

		// Novel chapters ship prose instead of page images.
		let text = data
			.content
			.map(|html| strip_html(&html))
			.unwrap_or_default();
		if text.is_empty() {
			bail!("No readable content found for {}", chapter.key);
		}
		Ok(vec![Page {
			content: PageContent::text(text),
			..Default::default()
		}])
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

impl Home for ValirScans {
	fn get_home(&self) -> Result<HomeLayout> {
		let payload = fetch(&format!("{DOMAIN}/"), false)?;
		let mut components: Vec<HomeComponent> = Vec::new();

		let featured: Vec<Series> =
			extract_all_by_marker::<Vec<Series>>(&payload, "\"initialSlides\":", false)
				.into_iter()
				.next()
				.unwrap_or_default();
		if !featured.is_empty() {
			components.push(HomeComponent {
				title: Some("Featured".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: featured.into_iter().map(series_to_manga).collect(),
					auto_scroll_interval: Some(6.0),
				},
			});
		}

		// The homepage embeds several series rows; the largest is the update feed.
		let mut lists: Vec<Vec<Series>> = extract_all_by_marker(&payload, "\"series\":", false);
		lists.retain(|list| !list.is_empty() && list.iter().all(|s| !s.title.is_empty()));
		lists.sort_by_key(|list| core::cmp::Reverse(list.len()));

		if let Some(latest) = lists.first() {
			let entries: Vec<MangaWithChapter> = latest
				.iter()
				.filter_map(|series| {
					let chapter = series.chapters.as_ref()?.first()?.clone();
					let manga = series_to_manga(series.clone());
					Some(MangaWithChapter {
						manga,
						chapter: Chapter {
							key: chapter.id,
							chapter_number: Some(chapter.number),
							date_uploaded: parse_iso(chapter.published_at.as_deref()),
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
						listing: None,
					},
				});
			}
		}

		for (title, list) in ["Popular", "Editor's Picks"]
			.iter()
			.zip(lists.iter().skip(1))
		{
			let entries: Vec<Link> = list.iter().cloned().map(series_to_link).collect();
			if entries.is_empty() {
				continue;
			}
			let ranked = *title == "Popular";
			components.push(HomeComponent {
				title: Some((*title).into()),
				subtitle: None,
				value: if ranked {
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

		if components.is_empty() {
			let entries: Vec<Link> = browse(1)?.into_iter().map(series_to_link).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("All Series".into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: Some(Listing {
							id: "browse".into(),
							name: "All Series".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for ValirScans {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		if listing.id != "browse" {
			bail!("Unknown listing");
		}
		let series = browse(page)?;
		let has_next_page = !series.is_empty();
		Ok(MangaPageResult {
			entries: series.into_iter().map(series_to_manga).collect(),
			has_next_page,
		})
	}
}

impl aidoku::ImageRequestProvider for ValirScans {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", &format!("{DOMAIN}/")))
	}
}

impl DeepLinkHandler for ValirScans {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/series/") else {
			return Ok(None);
		};
		let mut segments = url[idx + "/series/".len()..].split('/');
		let Some(kind) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};
		let Some(slug) = segments
			.next()
			.map(|s| s.split(['?', '#']).next().unwrap_or(s))
			.filter(|s| !s.is_empty())
		else {
			return Ok(None);
		};
		let manga_key = format!("{kind}/{slug}");

		if segments.next() == Some("chapter")
			&& let Some(id) = segments.next().filter(|s| !s.is_empty())
		{
			return Ok(Some(DeepLinkResult::Chapter {
				manga_key,
				key: id.split(['?', '#']).next().unwrap_or(id).to_string(),
			}));
		}
		Ok(Some(DeepLinkResult::Manga { key: manga_key }))
	}
}

register_source!(
	ValirScans,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
