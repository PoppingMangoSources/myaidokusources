#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::defaults::defaults_get,
	imports::html::{Document, Element},
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};

const DEFAULT_BASE_URL: &str = "https://vymanga.com";
const BASE_URL_KEY: &str = "baseUrl";

fn base_url() -> String {
	defaults_get::<String>(BASE_URL_KEY)
		.map(|url| url.trim().trim_end_matches('/').to_string())
		.filter(|url| url.starts_with("http"))
		.unwrap_or_else(|| DEFAULT_BASE_URL.into())
}

const ADULT_GENRES: &[&str] = &["adult", "hentai", "smut"];

fn abs_url(value: &str) -> String {
	let value = value.trim();
	if value.is_empty() {
		String::new()
	} else if let Some(rest) = value.strip_prefix("//") {
		format!("https://{rest}")
	} else if value.starts_with("http") {
		value.to_string()
	} else if value.starts_with('/') {
		format!("{}{value}", base_url())
	} else {
		format!("{}/{value}", base_url())
	}
}

fn img_from(el: &Element) -> String {
	let src = el
		.attr("data-src")
		.or_else(|| el.attr("data-lazy-src"))
		.or_else(|| el.attr("data-cfsrc"))
		.or_else(|| el.attr("src"))
		.unwrap_or_default();
	abs_url(&src)
}

fn manga_id_from(href: &str) -> Option<String> {
	let idx = href.find("/manga/")?;
	let after = &href[idx + "/manga/".len()..];
	let id = after.split(['/', '?', '#']).next().unwrap_or("");
	(!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
		.then(|| id.to_string())
}

fn chapter_number_from(text: &str) -> Option<f32> {
	let lower = text.to_lowercase();
	let search = lower
		.find("chapter")
		.map(|i| &lower[i + 7..])
		.unwrap_or(&lower);
	let mut num = String::new();
	for c in search.chars() {
		if c.is_ascii_digit() || c == '.' {
			num.push(c);
		} else if !num.is_empty() {
			break;
		}
	}
	num.trim_matches('.').parse::<f32>().ok()
}

fn content_rating_for(genres: &[String]) -> ContentRating {
	let lowered: Vec<String> = genres.iter().map(|g| g.trim().to_lowercase()).collect();
	if lowered.iter().any(|g| ADULT_GENRES.contains(&g.as_str())) {
		ContentRating::NSFW
	} else {
		ContentRating::Suggestive
	}
}

fn status_from(text: &str) -> MangaStatus {
	let s = text.to_lowercase();
	if s.contains("complet") || s.contains("finish") {
		MangaStatus::Completed
	} else if s.contains("ongoing") || s.contains("going") || s.contains("updating") {
		MangaStatus::Ongoing
	} else if s.contains("hiatus") || s.contains("pause") {
		MangaStatus::Hiatus
	} else if s.contains("cancel") || s.contains("drop") {
		MangaStatus::Cancelled
	} else {
		MangaStatus::Unknown
	}
}

fn parse_cards(doc: &Document) -> Vec<Manga> {
	let mut items = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	let Some(cards) = doc.select(".comic-item") else {
		return items;
	};
	for card in cards {
		let Some(link) = card.select_first("a") else {
			continue;
		};
		let href = link.attr("href").unwrap_or_default();
		let Some(manga_id) = manga_id_from(&href) else {
			continue;
		};
		if seen.iter().any(|s| s == &manga_id) {
			continue;
		}
		let title = card
			.select_first(".comic-title")
			.and_then(|el| el.text())
			.or_else(|| link.attr("title"))
			.unwrap_or_default();
		let title = title.trim().to_string();
		if title.is_empty() {
			continue;
		}
		seen.push(manga_id.clone());
		let cover = card
			.select_first(".comic-image img, img.image, img.lozad")
			.map(|img| img_from(&img));
		items.push(Manga {
			key: manga_id,
			title,
			cover,
			content_rating: ContentRating::Suggestive,
			..Default::default()
		});
	}
	items
}

fn has_next_page(doc: &Document) -> bool {
	doc.select_first("[rel=next]").is_some()
}

fn fetch_cards(url: &str) -> Result<MangaPageResult> {
	let doc = Request::get(url)?.html()?;
	let has_next_page = has_next_page(&doc);
	Ok(MangaPageResult {
		entries: parse_cards(&doc),
		has_next_page,
	})
}

fn browse_url(sort: &str, page: i32) -> String {
	let mut qs = QueryParameters::new();
	qs.push("sort", Some(sort));
	qs.push("sort_type", Some("desc"));
	qs.push("page", Some(&page.to_string()));
	format!("{}/search?{qs}", base_url())
}

struct VyManga;

impl Source for VyManga {
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

		let mut sort_index = 0;
		let mut author = String::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort_index = index,
				FilterValue::Text { id, value } if id == "author" => author = value,
				_ => {}
			}
		}

		let mut qs = QueryParameters::new();
		qs.push("q", Some(query));
		qs.push("page", Some(&page.to_string()));
		qs.push("search_po", Some("0"));
		qs.push("author_po", Some("0"));
		if !author.is_empty() {
			qs.push("author", Some(&author));
		}
		let sort = match sort_index {
			1 => Some("updated_at"),
			2 => Some("scored"),
			3 => Some("created_at"),
			_ => None,
		};
		if let Some(sort) = sort {
			qs.push("sort", Some(sort));
			qs.push("sort_type", Some("desc"));
		}
		fetch_cards(&format!("{}/search?{qs}", base_url()))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{}/manga/{}", base_url(), manga.key);
		let doc = Request::get(&url)?.html()?;

		if needs_details {
			manga.title = doc
				.select_first("h1")
				.and_then(|el| el.text())
				.map(|t| t.trim().to_string())
				.filter(|t| !t.is_empty())
				.unwrap_or_else(|| manga.key.clone());
			manga.cover = doc
				.select_first(".img-manga img, .content-thumb img")
				.map(|img| img_from(&img));
			manga.description = doc
				.select(".summary > .content, div.summary p.content")
				.and_then(|els| {
					let text = els
						.filter_map(|el| el.text())
						.map(|t| t.trim().to_string())
						.filter(|t| !t.is_empty())
						.collect::<Vec<_>>()
						.join("\n");
					(!text.is_empty()).then_some(text)
				});
			manga.authors = collect_text(&doc, ".pre-title:contains(Author) ~ a");
			manga.artists = collect_text(&doc, ".pre-title:contains(Artist) ~ a");

			let genres: Vec<String> = doc
				.select(".pre-title:contains(Genres) ~ a, div.col-md-7 p a[href*=genre]")
				.map(|els| {
					els.filter_map(|el| el.text())
						.map(|t| t.trim().to_string())
						.filter(|t| !t.is_empty())
						.collect()
				})
				.unwrap_or_default();
			manga.content_rating = content_rating_for(&genres);
			manga.tags = (!genres.is_empty()).then_some(genres);

			let status_text = doc
				.select(
					".pre-title:contains(Status) ~ span:not(.space), div.col-md-7 p:contains(Status) span",
				)
				.and_then(|els| els.into_iter().next_back())
				.and_then(|el| el.text())
				.unwrap_or_default();
			manga.status = status_from(&status_text);
			manga.url = Some(url.clone());

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();
			let mut seen: Vec<String> = Vec::new();
			let elements = doc
				.select("a.list-chapter")
				.filter(|els| !els.is_empty())
				.or_else(|| doc.select("a[id^=chapter-]"));
			if let Some(elements) = elements {
				for el in elements {
					let href = el
						.attr("href")
						.or_else(|| el.select_first("a").and_then(|a| a.attr("href")))
						.unwrap_or_default();
					if href.is_empty() {
						continue;
					}
					let key = abs_url(&href);
					if seen.iter().any(|s| s == &key) {
						continue;
					}
					seen.push(key.clone());
					let title = el
						.select_first("span")
						.and_then(|s| s.text())
						.or_else(|| el.select_first("p:not(.small)").and_then(|p| p.text()))
						.or_else(|| el.text())
						.unwrap_or_default();
					let title = title.trim().to_string();
					let chapter_number = chapter_number_from(&title);
					let date_uploaded = el
						.select_first("p.small")
						.and_then(|d| d.text())
						.and_then(|t| parse_relative_date(&t));
					chapters.push(Chapter {
						title: (!title.is_empty()).then_some(title),
						chapter_number,
						date_uploaded,
						url: Some(key.clone()),
						key,
						language: Some("en".into()),
						..Default::default()
					});
				}
			}
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let doc = Request::get(&chapter.key)?.html()?;
		let mut pages = Vec::new();
		let mut seen: Vec<String> = Vec::new();
		if let Some(imgs) = doc.select("div.carousel-item[data-page] img, img.lozad, img.d-block") {
			for img in imgs {
				let url = img_from(&img);
				let lower = url.to_lowercase();
				if url.is_empty()
					|| seen.iter().any(|s| s == &url)
					|| lower.contains("loading.gif")
					|| lower.contains("/logo")
					|| lower.contains("/icon")
					|| lower.contains("/avatar")
					|| lower.contains("/banner")
				{
					continue;
				}
				seen.push(url.clone());
				pages.push(Page {
					content: PageContent::url(url),
					..Default::default()
				});
			}
		}
		if pages.is_empty() {
			bail!("No pages found");
		}
		Ok(pages)
	}
}

fn collect_text(doc: &Document, selector: &str) -> Option<Vec<String>> {
	let values: Vec<String> = doc
		.select(selector)
		.map(|els| {
			els.filter_map(|el| el.text())
				.map(|t| t.trim().to_string())
				.filter(|t| {
					!t.is_empty()
						&& t != "-" && t.to_lowercase() != "n/a"
						&& t.to_lowercase() != "updating"
				})
				.collect()
		})
		.unwrap_or_default();
	(!values.is_empty()).then_some(values)
}

fn parse_relative_date(text: &str) -> Option<i64> {
	let trimmed = text.trim();
	if trimmed.is_empty() {
		return None;
	}
	parse_date(trimmed, "MMM d, yyyy").or_else(|| parse_date(trimmed, "MMMM d, yyyy"))
}

impl Home for VyManga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok(doc) = Request::get(browse_url("updated_at", 1))?.html() {
			let entries: Vec<Manga> = parse_cards(&doc).into_iter().take(15).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Latest Updated".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(5.0),
					},
				});
			}
		}

		for (title, sort) in [("Top Rated", "scored"), ("Newest", "created_at")] {
			if let Ok(doc) = Request::get(browse_url(sort, 1))?.html() {
				let entries: Vec<Link> = parse_cards(&doc)
					.into_iter()
					.map(|manga| Link {
						title: manga.title.clone(),
						subtitle: None,
						image_url: manga.cover.clone(),
						value: Some(LinkValue::Manga(manga)),
					})
					.collect();
				if !entries.is_empty() {
					let listing = Some(Listing {
						id: sort.into(),
						name: title.into(),
						..Default::default()
					});
					let value = if sort == "scored" {
						HomeComponentValue::MangaList {
							ranking: true,
							page_size: Some(10),
							entries,
							listing,
						}
					} else {
						HomeComponentValue::Scroller { entries, listing }
					};
					components.push(HomeComponent {
						title: Some(title.into()),
						subtitle: None,
						value,
					});
				}
			}
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for VyManga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let sort = match listing.id.as_str() {
			"updated_at" => "updated_at",
			"scored" => "scored",
			"created_at" => "created_at",
			_ => bail!("Unknown listing"),
		};
		fetch_cards(&browse_url(sort, page.max(1)))
	}
}

impl aidoku::ImageRequestProvider for VyManga {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", &format!("{}/", base_url())))
	}
}

impl DeepLinkHandler for VyManga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(manga_id) = manga_id_from(&url) else {
			return Ok(None);
		};
		Ok(Some(DeepLinkResult::Manga { key: manga_id }))
	}
}

register_source!(
	VyManga,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
