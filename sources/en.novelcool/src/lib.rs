#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	UpdateStrategy, Viewer,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::encode_uri_component,
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::de::DeserializeOwned;

mod models;

use models::*;

const APP_ID: &str = "202201290625004";
const APP_SECRET: &str = "c73a8590641781f203660afca1d37ada";
const PACKAGE_NAME: &str = "com.zuoyou.novel";

const ADULT_GENRES: &[&str] = &["adult", "hentai", "smut", "yaoi"];
const MATURE_GENRES: &[&str] = &["ecchi", "mature", "yuri"];

/// Posts a form-encoded request to the mobile API and returns the parsed body.
fn api_post<T: DeserializeOwned>(path: &str, params: &[(&str, &str)]) -> Result<ApiResponse<T>> {
	let mut body =
		format!("appId={APP_ID}&secret={APP_SECRET}&package_name={PACKAGE_NAME}&lang=en");
	for (key, value) in params {
		body.push('&');
		body.push_str(key);
		body.push('=');
		body.push_str(&encode_uri_component(value));
	}

	let url = format!("{API_URL}/{}/", path.trim_matches('/'));
	let response: ApiResponse<T> = Request::post(&url)?
		.header("Content-Type", "application/x-www-form-urlencoded")
		.header("User-Agent", USER_AGENT)
		.body(body)
		.send()?
		.get_json_owned()?;

	if response.error_code.as_deref() != Some("success") {
		let message = response
			.error_msg
			.as_deref()
			.unwrap_or("API request failed");
		bail!("{message}");
	}
	Ok(response)
}

fn resolve_url(value: &str) -> String {
	let trimmed = value.trim();
	if trimmed.is_empty() {
		String::new()
	} else if let Some(rest) = trimmed.strip_prefix("//") {
		format!("https://{rest}")
	} else if trimmed.starts_with("http") {
		trimmed.to_string()
	} else if trimmed.starts_with('/') {
		format!("{DOMAIN}{trimmed}")
	} else {
		format!("{DOMAIN}/{trimmed}")
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

fn status_from(completed: Option<&str>) -> MangaStatus {
	match completed {
		Some("YES") => MangaStatus::Completed,
		Some("NO") => MangaStatus::Ongoing,
		_ => MangaStatus::Unknown,
	}
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

fn parse_api_date(value: Option<&str>) -> Option<i64> {
	let text = value?.trim();
	if text.len() < 19 {
		return None;
	}
	parse_date(&text[..19], "yyyy-MM-dd HH:mm:ss")
}

fn split_creators(value: Option<&str>) -> Option<Vec<String>> {
	let raw = value?.trim();
	if raw.is_empty() || raw.eq_ignore_ascii_case("updating") {
		return None;
	}
	let names: Vec<String> = raw
		.split(',')
		.map(|n| n.trim().to_string())
		.filter(|n| !n.is_empty())
		.collect();
	(!names.is_empty()).then_some(names)
}

fn book_to_manga(book: Book) -> Manga {
	let genres = book.category_list.clone().unwrap_or_default();
	let is_novel = book.is_novel();
	let content_rating = content_rating_for(&genres);
	Manga {
		key: book.key().to_string(),
		title: book.name.trim().to_string(),
		cover: Some(resolve_url(&book.cover)),
		description: book
			.intro
			.as_deref()
			.map(strip_html)
			.filter(|d| !d.is_empty()),
		authors: split_creators(book.author.as_deref()),
		artists: split_creators(book.artist.as_deref()),
		status: status_from(book.completed.as_deref()),
		content_rating,
		viewer: if is_novel {
			Viewer::Vertical
		} else {
			Viewer::RightToLeft
		},
		tags: (!genres.is_empty()).then_some(genres),
		url: book.url.as_deref().map(resolve_url),
		..Default::default()
	}
}

fn browse(order: &str, content_type: &str, page: i32) -> Result<Vec<Book>> {
	let response: ApiResponse<Book> = api_post(
		&format!("elite/{order}"),
		&[
			("lc_type", content_type),
			("page", &page.to_string()),
			("page_size", &PAGE_SIZE.to_string()),
		],
	)?;
	Ok(response.list.unwrap_or_default())
}

/// Fetches a listing page, merging both content types unless one is requested.
fn browse_merged(order: &str, content_type: &str, page: i32) -> Result<MangaPageResult> {
	let mut books = Vec::new();
	if content_type == "all" || content_type == "novel" {
		books.extend(browse(order, "novel", page)?);
	}
	if content_type == "all" || content_type == "manga" {
		books.extend(browse(order, "manga", page)?);
	}
	let has_next_page = !books.is_empty();
	Ok(MangaPageResult {
		entries: books.into_iter().map(book_to_manga).collect(),
		has_next_page,
	})
}

struct NovelCool;

impl Source for NovelCool {
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

		let mut content_type = "all";
		for filter in filters {
			if let FilterValue::Select { id, value } = filter
				&& id == "type"
			{
				content_type = match value.as_str() {
					"manga" => "manga",
					"novel" => "novel",
					_ => "all",
				};
			}
		}

		if query.is_empty() {
			return browse_merged("hot", content_type, page);
		}

		let mut books = Vec::new();
		for kind in ["novel", "manga"] {
			if content_type != "all" && content_type != kind {
				continue;
			}
			let response: ApiResponse<Book> = api_post(
				"book/search",
				&[
					("keyword", query),
					("lc_type", kind),
					("page", &page.to_string()),
					("page_size", &PAGE_SIZE.to_string()),
				],
			)?;
			books.extend(response.list.unwrap_or_default());
		}

		let has_next_page = !books.is_empty();
		Ok(MangaPageResult {
			entries: books.into_iter().map(book_to_manga).collect(),
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let book_id = manga.key.clone();
		let mut is_novel = manga.viewer == Viewer::Vertical;

		if needs_details {
			let response: ApiResponse<Book> = api_post("book/info", &[("book_id", &book_id)])?;
			let book = response.info.ok_or_else(|| error!("No book info"))?;
			is_novel = book.is_novel();
			let mut details = book_to_manga(book);
			details.key = book_id.clone();
			manga.copy_from(details);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let response: ApiResponse<ApiChapter> =
				api_post("chapter/book_list", &[("book_id", &book_id)])?;
			let list = response.list.unwrap_or_default();

			let mut chapters: Vec<Chapter> = list
				.into_iter()
				.filter(|c| !is_locked(c.is_locked.as_ref()))
				.enumerate()
				.map(|(index, item)| {
					let chapter_number = item
						.order_id
						.as_deref()
						.and_then(|o| o.parse::<f32>().ok())
						.filter(|n| *n > 0.0)
						.or_else(|| chapter_number_from(&item.title))
						.unwrap_or((index + 1) as f32);
					Chapter {
						key: item.id,
						title: chapter_title_from(&item.title),
						chapter_number: Some(chapter_number),
						date_uploaded: parse_api_date(
							item.last_modify.as_deref().or(item.tf_time.as_deref()),
						),
						language: Some("en".into()),
						..Default::default()
					}
				})
				.collect();
			chapters.reverse();
			manga.chapters = Some(chapters);
		}

		if is_novel {
			manga.viewer = Viewer::Vertical;
			manga.update_strategy = UpdateStrategy::Always;
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let response: ApiResponse<ApiChapter> =
			api_post("chapter/info", &[("chapter_id", &chapter.key)])?;
		let info = response.info.ok_or_else(|| error!("No chapter content"))?;

		if is_locked(info.is_locked.as_ref()) {
			bail!("This chapter is locked and cannot be read");
		}

		// Comics return an image list; novels return html content instead.
		let mut pics = info.pic_list.unwrap_or_default();
		if !pics.is_empty() {
			pics.sort_by_key(|p| p.order_id);
			let pages: Vec<Page> = pics
				.into_iter()
				.map(|p| resolve_url(&p.pic_path))
				.filter(|url| url.starts_with("http"))
				.map(|url| Page {
					content: PageContent::url(url),
					..Default::default()
				})
				.collect();
			if !pages.is_empty() {
				return Ok(pages);
			}
		}

		let content = info.content.unwrap_or_default();
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

fn chapter_number_from(title: &str) -> Option<f32> {
	let lower = title.to_lowercase();
	let idx = ["chapter", "chap", "ch.", "episode", "ep.", "part"]
		.iter()
		.find_map(|kw| lower.find(kw).map(|i| i + kw.len()))?;
	let mut num = String::new();
	for c in lower[idx..].chars() {
		if c.is_ascii_digit() || c == '.' {
			num.push(c);
		} else if !num.is_empty() {
			break;
		} else if c != ' ' {
			return None;
		}
	}
	num.trim_matches('.').parse::<f32>().ok()
}

fn chapter_title_from(title: &str) -> Option<String> {
	let trimmed = title.trim();
	if trimmed.is_empty() {
		return None;
	}
	// Drop a leading "Chapter N" prefix so only a real title remains.
	let lower = trimmed.to_lowercase();
	for kw in ["chapter", "chap.", "chap", "ch.", "episode", "ep.", "part"] {
		if let Some(rest) = lower.strip_prefix(kw) {
			let offset = trimmed.len() - rest.len();
			let after: &str = &trimmed[offset..];
			let after = after
				.trim_start()
				.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
				.trim_start_matches([' ', '-', ':', '–', '—'])
				.trim();
			return (!after.is_empty()).then(|| after.to_string());
		}
	}
	Some(trimmed.to_string())
}

fn book_to_link(book: Book) -> Link {
	let manga = book_to_manga(book);
	Link {
		title: manga.title.clone(),
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

impl Home for NovelCool {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok(books) = browse("hot", "manga", 1) {
			let entries: Vec<Manga> = books.into_iter().take(10).map(book_to_manga).collect();
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

		if let Ok(books) = browse("latest", "manga", 1) {
			let entries: Vec<MangaWithChapter> = books
				.into_iter()
				.filter_map(|book| {
					let chapter_id = book.last_chapter_id.clone()?;
					let chapter_title = book.last_chapter_title.clone();
					let date_uploaded = parse_api_date(book.modify_time.as_deref());
					let manga = book_to_manga(book);
					Some(MangaWithChapter {
						manga,
						chapter: Chapter {
							key: chapter_id,
							chapter_number: chapter_title.as_deref().and_then(chapter_number_from),
							title: chapter_title,
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
							id: "latest".into(),
							name: "Latest Updates".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		for (title, order, kind) in [
			("New Releases", "new_book", "manga"),
			("Popular Novels", "hot", "novel"),
		] {
			if let Ok(books) = browse(order, kind, 1) {
				let entries: Vec<Link> = books.into_iter().map(book_to_link).collect();
				if !entries.is_empty() {
					components.push(HomeComponent {
						title: Some(title.into()),
						subtitle: None,
						value: HomeComponentValue::Scroller {
							entries,
							listing: Some(Listing {
								id: order.into(),
								name: title.into(),
								..Default::default()
							}),
						},
					});
				}
			}
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for NovelCool {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let order = match listing.id.as_str() {
			"hot" => "hot",
			"latest" => "latest",
			"new_book" => "new_book",
			_ => bail!("Unknown listing"),
		};
		browse_merged(order, "all", page.max(1))
	}
}

impl aidoku::ImageRequestProvider for NovelCool {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for NovelCool {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// ex: https://www.novelcool.com/novel/Some-Title.html
		let Some(idx) = url.find("/novel/") else {
			return Ok(None);
		};
		let after = &url[idx + "/novel/".len()..];
		let slug = after
			.split(['/', '?', '#'])
			.next()
			.unwrap_or("")
			.trim_end_matches(".html");
		if slug.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga {
			key: slug.to_string(),
		}))
	}
}

register_source!(
	NovelCool,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
