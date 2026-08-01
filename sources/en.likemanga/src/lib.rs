#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, MangaWithChapter, Page, PageContent, PageContext, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::html::{Document, Element, Html},
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use base64::{Engine, engine::general_purpose};
use serde::Deserialize;

const BASE_URL: &str = "https://likemanga.ink";

const ADULT_GENRES: &[&str] = &["adult", "hentai", "smut"];
const MATURE_GENRES: &[&str] = &["ecchi", "mature", "yaoi", "yuri"];

#[derive(Deserialize)]
struct ChapterAjaxResponse {
	list_chap: Option<String>,
}

/// Keys are site paths, so links survive whichever domain is in use.
fn path_of(href: &str) -> String {
	let trimmed = href.trim();
	let without_host = trimmed
		.split_once("://")
		.map(|(_, rest)| rest.split_once('/').map(|(_, p)| p).unwrap_or(""))
		.unwrap_or(trimmed);
	without_host
		.split(['?', '#'])
		.next()
		.unwrap_or("")
		.trim_matches('/')
		.to_string()
}

fn url_for(path: &str) -> String {
	format!("{BASE_URL}/{}", path.trim_matches('/'))
}

fn clean(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn img_from(el: &Element) -> Option<String> {
	el.attr("abs:data-cfsrc")
		.or_else(|| el.attr("abs:data-src"))
		.or_else(|| el.attr("abs:data-lazy-src"))
		.or_else(|| el.attr("abs:src"))
		.filter(|url| !url.is_empty())
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

fn status_from(text: &str) -> MangaStatus {
	let s = text.to_lowercase();
	if s.contains("complete") {
		MangaStatus::Completed
	} else if s.contains("in process") || s.contains("ongoing") {
		MangaStatus::Ongoing
	} else if s.contains("pause") || s.contains("hiatus") {
		MangaStatus::Hiatus
	} else {
		MangaStatus::Unknown
	}
}

fn chapter_number_from(name: &str) -> Option<f32> {
	let lower = name.to_lowercase();
	let start = ["chapter", "ch."]
		.iter()
		.find_map(|kw| lower.find(kw).map(|i| i + kw.len()))?;
	let mut digits = String::new();
	for c in lower[start..].chars() {
		if c.is_ascii_digit() || c == '.' {
			digits.push(c);
		} else if !digits.is_empty() {
			break;
		} else if c != ' ' {
			return None;
		}
	}
	digits.trim_matches('.').parse::<f32>().ok()
}

fn parse_site_date(text: &str) -> Option<i64> {
	let trimmed = text.trim();
	if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("new") {
		return None;
	}
	parse_date(trimmed, "dd/MM/yyyy")
		.or_else(|| parse_date(trimmed, "MMM d, yyyy"))
		.or_else(|| parse_date(trimmed, "yyyy-MM-dd"))
}

fn parse_cards(doc: &Document) -> Vec<(Manga, Option<(String, String)>)> {
	let mut items = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	let Some(cards) = doc.select(".video") else {
		return items;
	};
	for card in cards {
		let link = card
			.select_first("p.title-manga a")
			.or_else(|| card.select_first("a"));
		let Some(link) = link else { continue };
		let href = link.attr("href").unwrap_or_default();
		let key = path_of(&href);
		if key.is_empty() || seen.iter().any(|s| s == &key) {
			continue;
		}
		let title = link
			.text()
			.map(|t| clean(&t))
			.filter(|t| !t.is_empty())
			.or_else(|| card.select_first("img").and_then(|img| img.attr("alt")))
			.unwrap_or_default();
		if title.is_empty() {
			continue;
		}
		seen.push(key.clone());

		let cover = card.select_first("img").and_then(|img| img_from(&img));
		let tooltip = card
			.select_first("img[data-jtip]")
			.and_then(|img| img.attr("data-jtip"))
			.and_then(|selector| doc.select_first(&selector));
		let description = tooltip
			.as_ref()
			.and_then(|tip| tip.select_first(".box_text"))
			.and_then(|text| text.text())
			.map(|text| clean(&text))
			.filter(|text| !text.is_empty());
		let mut genres: Vec<String> = Vec::new();
		let mut status = MangaStatus::Unknown;
		if let Some(tip) = tooltip
			&& let Some(rows) = tip.select(".message_main p")
		{
			for row in rows {
				let text = row.text().map(|text| clean(&text)).unwrap_or_default();
				if let Some(value) = text.strip_prefix("Genres:") {
					genres = value
						.split(',')
						.map(|genre| genre.trim().to_string())
						.filter(|genre| !genre.is_empty())
						.collect();
				} else if let Some(value) = text.strip_prefix("Status:") {
					status = status_from(value.trim());
				}
			}
		}
		let latest = card.select_first(".list-group-item a").and_then(|a| {
			let chapter_href = a.attr("href").unwrap_or_default();
			let chapter_key = path_of(&chapter_href);
			let name = a.text().map(|t| clean(&t)).unwrap_or_default();
			(!chapter_key.is_empty()).then_some((chapter_key, name))
		});

		items.push((
			Manga {
				key,
				title,
				cover,
				description,
				status,
				content_rating: content_rating_for(&genres),
				tags: (!genres.is_empty()).then_some(genres),
				..Default::default()
			},
			latest,
		));
	}
	items
}

fn fetch_cards(url: &str) -> Result<MangaPageResult> {
	let doc = Request::get(url)?
		.header("Referer", &format!("{BASE_URL}/"))
		.html()?;
	let entries: Vec<Manga> = parse_cards(&doc).into_iter().map(|(m, _)| m).collect();
	let has_next_page = !entries.is_empty();
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

fn manga_link(manga: Manga) -> Link {
	Link {
		title: manga.title.clone(),
		subtitle: manga.tags.as_ref().map(|tags| tags.join(" · ")),
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn parse_new_manga(doc: &Document) -> Vec<Link> {
	let mut links = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	let Some(items) = doc.select(".items-slide .item") else {
		return links;
	};
	for item in items {
		let Some(link) = item.select_first(".slide-caption h3 a") else {
			continue;
		};
		let href = link.attr("href").unwrap_or_default();
		let key = path_of(&href);
		let title = link.text().map(|text| clean(&text)).unwrap_or_default();
		if key.is_empty() || title.is_empty() || seen.iter().any(|value| value == &key) {
			continue;
		}
		seen.push(key.clone());
		let subtitle = item
			.select_first(".slide-caption > a")
			.and_then(|chapter| chapter.text())
			.map(|text| clean(&text))
			.filter(|text| !text.is_empty());
		let manga = Manga {
			key,
			title: title.clone(),
			cover: item.select_first("img").and_then(|img| img_from(&img)),
			content_rating: ContentRating::Safe,
			..Default::default()
		};
		links.push(Link {
			title,
			subtitle,
			image_url: manga.cover.clone(),
			value: Some(LinkValue::Manga(manga)),
		});
	}
	links
}

fn search_url(
	keyword: &str,
	sort: &str,
	status: &str,
	min_chapters: &str,
	genres: &[String],
	page: i32,
) -> String {
	let mut qs = QueryParameters::new();
	qs.push("act", Some("searchadvance"));
	if !keyword.is_empty() {
		qs.push("f[keyword]", Some(keyword));
	}
	if !sort.is_empty() {
		qs.push("f[sortby]", Some(sort));
	}
	if !status.is_empty() {
		qs.push("f[status]", Some(status));
	}
	for genre in genres {
		qs.push("f[genres][]", Some(genre));
	}
	if !min_chapters.is_empty() {
		qs.push("f[min_num_chapter]", Some(min_chapters));
	}
	if page > 1 {
		qs.push("pageNum", Some(&page.to_string()));
	}
	format!("{BASE_URL}/?{qs}")
}

fn sort_id(index: i32) -> &'static str {
	match index {
		1 => "lastest-manga",
		2 => "top-manga",
		3 => "top-month",
		4 => "top-week",
		5 => "top-day",
		6 => "follow",
		7 => "comment",
		8 => "num-chap",
		_ => "lastest-chap",
	}
}

struct LikeManga;

impl Source for LikeManga {
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
		let keyword = query.trim();

		let mut sort = "lastest-chap";
		let mut status = String::new();
		let mut min_chapters = String::new();
		let mut genres: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = sort_id(index),
				FilterValue::Select { id, value } if id == "status" => status = value,
				FilterValue::Select { id, value } if id == "min_chapters" => min_chapters = value,
				FilterValue::MultiSelect { id, included, .. } if id == "genres" => {
					genres = included
				}
				_ => {}
			}
		}

		fetch_cards(&search_url(
			keyword,
			sort,
			&status,
			&min_chapters,
			&genres,
			page.max(1),
		))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = url_for(&manga.key);
		let doc = Request::get(&url)?
			.header("Referer", &format!("{BASE_URL}/"))
			.html()?;

		let title_el = doc.select_first("#title-detail-manga");
		let numeric_id = title_el.as_ref().and_then(|el| el.attr("data-manga"));

		if needs_details {
			manga.title = title_el
				.as_ref()
				.and_then(|el| el.text())
				.map(|t| clean(&t))
				.filter(|t| !t.is_empty())
				.unwrap_or_else(|| manga.key.clone());
			manga.cover = doc
				.select_first(".detail-info img")
				.and_then(|img| img_from(&img));
			manga.description = doc
				.select_first("#summary_shortened")
				.and_then(|el| el.text())
				.map(|t| t.trim().to_string())
				.filter(|t| !t.is_empty());

			let genres: Vec<String> = doc
				.select(".list-info .kind a")
				.map(|els| {
					els.filter_map(|el| el.text())
						.map(|t| clean(&t))
						.filter(|t| !t.is_empty())
						.collect()
				})
				.unwrap_or_default();
			manga.content_rating = content_rating_for(&genres);
			manga.tags = (!genres.is_empty()).then_some(genres);

			manga.authors = doc
				.select_first(".list-info .author p:nth-child(2)")
				.and_then(|el| el.text())
				.map(|t| clean(&t))
				.filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("updating"))
				.map(|a| vec![a]);
			manga.status = doc
				.select_first(".list-info .status p:nth-child(2)")
				.and_then(|el| el.text())
				.map(|t| status_from(&t))
				.unwrap_or_default();
			manga.url = Some(url.clone());

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut rows: Vec<Chapter> = Vec::new();
			let mut seen: Vec<String> = Vec::new();
			collect_chapters(&doc, &mut rows, &mut seen);

			// Older chapters live behind an ajax-paginated list.
			if let Some(numeric_id) = numeric_id {
				let last_page = doc
					.select(".chapters_pagination a")
					.map(|els| {
						els.filter_map(|a| a.attr("onclick"))
							.filter_map(|onclick| {
								let start = onclick.find("load_list_chapter(")? + 18;
								let rest = &onclick[start..];
								let end = rest.find(')')?;
								rest[..end].trim().parse::<i32>().ok()
							})
							.max()
							.unwrap_or(1)
					})
					.unwrap_or(1);

				for page in 2..=last_page.min(50) {
					let mut qs = QueryParameters::new();
					qs.push("act", Some("ajax"));
					qs.push("code", Some("load_list_chapter"));
					qs.push("manga_id", Some(&numeric_id));
					qs.push("page_num", Some(&page.to_string()));
					qs.push("chap_id", Some("0"));
					qs.push("keyword", Some(""));
					let Ok(response) = Request::get(format!("{BASE_URL}/?{qs}"))?
						.header("Referer", &url)
						.send()
					else {
						break;
					};
					let Ok(json) = response.get_json_owned::<ChapterAjaxResponse>() else {
						break;
					};
					let Some(fragment) = json.list_chap else {
						break;
					};
					if let Ok(doc) = Html::parse_fragment(&fragment) {
						collect_chapters(&doc, &mut rows, &mut seen);
					}
				}
			}

			if rows.is_empty() {
				bail!("No chapters found");
			}
			manga.chapters = Some(rows);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = url_for(&chapter.key);
		let doc = Request::get(&url)?
			.header("Referer", &format!("{BASE_URL}/"))
			.html()?;

		// The reader ships its image manifest inside a token value.
		let token = doc
			.select_first("#next_img_token")
			.and_then(|el| el.attr("value"))
			.unwrap_or_default();
		let cdn = doc
			.select_first("#currentlink")
			.and_then(|el| el.attr("value"))
			.map(|url| url.trim_end_matches('/').to_string())
			.unwrap_or_default();

		if let (Some(payload), false) = (token.split('.').nth(1), cdn.is_empty())
			&& let Some(images) = decode_manifest(payload)
		{
			let pages: Vec<Page> = images
				.into_iter()
				.map(|image| Page {
					content: PageContent::url(format!("{cdn}/{image}")),
					..Default::default()
				})
				.collect();
			if !pages.is_empty() {
				return Ok(pages);
			}
		}

		let mut pages = Vec::new();
		let mut seen: Vec<String> = Vec::new();
		if let Some(imgs) = doc.select(".reading-detail.box_doc img") {
			for img in imgs {
				let Some(url) = img_from(&img) else { continue };
				if seen.iter().any(|s| s == &url) {
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
			bail!("No readable pages found");
		}
		Ok(pages)
	}
}

fn collect_chapters(doc: &Document, rows: &mut Vec<Chapter>, seen: &mut Vec<String>) {
	let Some(items) = doc.select(".wp-manga-chapter") else {
		return;
	};
	for row in items {
		let Some(link) = row.select_first("a") else {
			continue;
		};
		let key = path_of(&link.attr("href").unwrap_or_default());
		if key.is_empty() || seen.iter().any(|s| s == &key) {
			continue;
		}
		seen.push(key.clone());
		let name = link.text().map(|t| clean(&t)).unwrap_or_default();
		let chapter_number = chapter_number_from(&name);
		let date_uploaded = row
			.select_first(".chapter-release-date")
			.and_then(|el| el.text())
			.and_then(|t| parse_site_date(&t));
		rows.push(Chapter {
			url: Some(url_for(&key)),
			key,
			title: (!name.is_empty()).then_some(name),
			chapter_number,
			date_uploaded,
			language: Some("en".into()),
			..Default::default()
		});
	}
}

/// The token payload nests a second base64 blob holding the image list.
fn decode_manifest(payload: &str) -> Option<Vec<String>> {
	let outer = general_purpose::STANDARD_NO_PAD
		.decode(payload.trim_end_matches('='))
		.ok()?;
	let outer: serde_json::Value = serde_json::from_slice(&outer).ok()?;
	let data = outer.get("data")?.as_str()?;
	let inner = general_purpose::STANDARD_NO_PAD
		.decode(data.trim_end_matches('='))
		.ok()?;
	let manifest: serde_json::Value = serde_json::from_slice(&inner).ok()?;
	let images = manifest.as_array()?;
	Some(
		images
			.iter()
			.filter_map(|image| image.as_str())
			.filter(|image| !image.is_empty())
			.map(|image| image.to_string())
			.collect(),
	)
}

impl Home for LikeManga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		let doc = Request::get(BASE_URL)?
			.header("Referer", &format!("{BASE_URL}/"))
			.html()?;

		if let Ok(result) = fetch_cards(&search_url("", "follow", "", "", &[], 1)) {
			let entries: Vec<Manga> = result
				.entries
				.into_iter()
				.filter(|manga| manga.description.is_some())
				.take(8)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Most Followed".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(6.0),
					},
				});
			}
		}

		let new_manga = parse_new_manga(&doc);
		if !new_manga.is_empty() {
			components.push(HomeComponent {
				title: Some("New Manga".into()),
				subtitle: None,
				value: HomeComponentValue::Scroller {
					entries: new_manga,
					listing: Some(Listing {
						id: "lastest-manga".into(),
						name: "Newest".into(),
						..Default::default()
					}),
				},
			});
		}

		let latest = parse_cards(&doc);
		if !latest.is_empty() {
			let chapter_entries: Vec<MangaWithChapter> = latest
				.into_iter()
				.filter_map(|(manga, latest)| {
					let (key, name) = latest?;
					let chapter_number = chapter_number_from(&name);
					Some(MangaWithChapter {
						manga,
						chapter: Chapter {
							key,
							title: (!name.is_empty()).then_some(name),
							chapter_number,
							..Default::default()
						},
					})
				})
				.collect();
			if !chapter_entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Latest Releases".into()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: None,
						entries: chapter_entries,
						listing: Some(Listing {
							id: "lastest-chap".into(),
							name: "Latest Updates".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		if let Ok(result) = fetch_cards(&format!("{BASE_URL}/hot/")) {
			let entries: Vec<Link> = result.entries.into_iter().map(manga_link).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Hot".into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: Some(Listing {
							id: "hot".into(),
							name: "Hot".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		let mut seen: Vec<String> = Vec::new();
		let genres: Vec<FilterItem> = doc
			.select("a[href*='/genres/']")
			.map(|items| {
				items
					.filter_map(|link| {
						let href = link.attr("href")?;
						let genre = href.split("/genres/").nth(1)?.split('/').next()?;
						let title = link.text().map(|text| clean(&text)).unwrap_or_default();
						if genre.is_empty() || title.is_empty() || seen.iter().any(|id| id == genre)
						{
							return None;
						}
						seen.push(genre.into());
						Some(FilterItem {
							title,
							values: Some(vec![FilterValue::MultiSelect {
								id: "genres".into(),
								included: vec![genre.into()],
								excluded: Vec::new(),
							}]),
						})
					})
					.collect()
			})
			.unwrap_or_default();
		if !genres.is_empty() {
			components.push(HomeComponent {
				title: Some("Genres".into()),
				subtitle: None,
				value: HomeComponentValue::Filters(genres),
			});
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for LikeManga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let sort = match listing.id.as_str() {
			"top-manga" | "top-day" | "top-week" | "top-month" | "lastest-chap"
			| "lastest-manga" | "follow" => listing.id.as_str(),
			"hot" => return fetch_cards(&format!("{BASE_URL}/hot/")),
			_ => bail!("Unknown listing"),
		};
		fetch_cards(&search_url("", sort, "", "", &[], page.max(1)))
	}
}

impl aidoku::ImageRequestProvider for LikeManga {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{BASE_URL}/"))
			.header("Origin", BASE_URL))
	}
}

impl DeepLinkHandler for LikeManga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = path_of(&url);
		if path.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga { key: path }))
	}
}

register_source!(
	LikeManga,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
