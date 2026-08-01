#![no_std]
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, Listing, ListingProvider, Manga, MangaPageResult,
	MangaWithChapter, Page, PageContent, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::net::Request,
	imports::std::{parse_date, send_partial_result},
	prelude::*,
};
use serde::{Deserialize, de::DeserializeOwned};

mod models;
mod parser;

use models::*;
use parser::*;

const PAGE_SIZE: usize = 20;

#[derive(Deserialize, Default)]
struct ChapterPost {
	acf: Option<ChapterAcf>,
	date: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChapterAcf {
	chapter_number: Option<serde_json::Value>,
	novel_code: Option<serde_json::Value>,
	ch_name: Option<String>,
}

fn get_json<T: DeserializeOwned>(url: &str) -> Result<T> {
	Request::get(url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Accept", "application/json,*/*;q=0.8")
		.send()?
		.get_json_owned()
}

fn get_html(url: &str) -> Result<aidoku::imports::html::Document> {
	Ok(Request::get(url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.html()?)
}

/// Fetches the full novel catalogue, which the site serves in one page.
fn catalogue() -> Result<Vec<Novel>> {
	let raw: Vec<serde_json::Value> = get_json(&format!(
		"{CHAPTER_API}/wp-json/wp/v2/mvl-novels?per_page={CATALOGUE_PAGE_SIZE}&page=1"
	))?;
	Ok(raw.iter().filter_map(Novel::from_value).collect())
}

fn sorted_catalogue(sort: Sort) -> Result<Vec<Novel>> {
	let mut novels = catalogue()?;
	sort.apply(&mut novels);
	Ok(novels)
}

fn page_of(novels: Vec<Novel>, page: i32) -> MangaPageResult {
	let page = page.max(1) as usize;
	let start = (page - 1) * PAGE_SIZE;
	let has_next_page = novels.len() > start + PAGE_SIZE;
	MangaPageResult {
		entries: novels
			.into_iter()
			.skip(start)
			.take(PAGE_SIZE)
			.map(Manga::from)
			.collect(),
		has_next_page,
	}
}

struct Mvlempyr;

impl Source for Mvlempyr {
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

		let mut sort = Sort::Popular;
		let mut status = String::from("all");
		let mut included: Vec<String> = Vec::new();
		let mut excluded: Vec<String> = Vec::new();
		let mut match_all = true;
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = Sort::from_index(index),
				FilterValue::Select { id, value } if id == "status" => status = value,
				FilterValue::Select { id, value } if id == "genre_match" => {
					match_all = value != "or"
				}
				FilterValue::MultiSelect {
					id,
					included: inc,
					excluded: exc,
				} if id == "genres" => {
					included = inc;
					excluded = exc;
				}
				_ => {}
			}
		}

		let mut novels = sorted_catalogue(sort)?;
		novels.retain(|novel| {
			if !query.is_empty() && !novel.name.to_lowercase().contains(&query) {
				return false;
			}
			if status != "all" && !novel.status_matches(&status) {
				return false;
			}
			if !novel.genres_match(&included, &excluded, match_all) {
				return false;
			}
			true
		});

		Ok(page_of(novels, page))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = manga.key.clone();
		let html = get_html(&format!("{DOMAIN}/novel/{slug}"))?;
		let code = parse_novel_code(&html);

		if needs_details {
			let details = parse_novel_details(&html, &slug, code);
			manga.copy_from(details);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let Some(code) = code else {
				bail!("Unable to determine the novel code for {slug}");
			};
			let tag = chapter_tag_id(code);
			let mut chapters: Vec<Chapter> = Vec::new();
			let mut page = 1;
			loop {
				let posts: Vec<ChapterPost> = get_json(&format!(
					"{CHAPTER_API}/wp-json/wp/v2/posts?tags={tag}&per_page={CHAPTER_PAGE_SIZE}&page={page}"
				))
				.unwrap_or_default();
				if posts.is_empty() {
					break;
				}
				let count = posts.len();
				chapters.extend(posts.into_iter().filter_map(|post| build_chapter(&post)));
				if count < CHAPTER_PAGE_SIZE as usize || page >= 20 {
					break;
				}
				page += 1;
			}
			chapters.sort_by(|a, b| {
				a.chapter_number
					.partial_cmp(&b.chapter_number)
					.unwrap_or(core::cmp::Ordering::Equal)
			});
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let html = get_html(&format!("{DOMAIN}/chapter/{}", chapter.key))?;
		let text = parse_chapter_text(&html);
		if text.is_empty() {
			bail!("No readable content found");
		}
		Ok(vec![Page {
			content: PageContent::text(text),
			..Default::default()
		}])
	}
}

fn build_chapter(post: &ChapterPost) -> Option<Chapter> {
	let acf = post.acf.as_ref()?;
	let number = number_of(acf.chapter_number.as_ref())?;
	let code = number_of(acf.novel_code.as_ref())? as i64;
	let title = acf
		.ch_name
		.as_deref()
		.map(str::trim)
		.filter(|t| !t.is_empty())
		.map(|t| t.to_string());
	let date_uploaded = post.date.as_deref().and_then(|raw| {
		let trimmed = raw.trim();
		(trimmed.len() >= 19).then(|| parse_date(&trimmed[..19], "yyyy-MM-dd'T'HH:mm:ss"))?
	});
	Some(Chapter {
		key: format!("{code}-{}", format_number(number)),
		title,
		chapter_number: Some(number),
		date_uploaded,
		url: Some(format!("{DOMAIN}/chapter/{code}-{}", format_number(number))),
		language: Some("en".into()),
		..Default::default()
	})
}

impl Home for Mvlempyr {
	fn get_home(&self) -> Result<HomeLayout> {
		let novels = catalogue()?;
		let mut components: Vec<HomeComponent> = Vec::new();

		let take = |sort: Sort, count: usize| -> Vec<Novel> {
			let mut items = novels.clone();
			sort.apply(&mut items);
			items.into_iter().take(count).collect()
		};

		let popular = take(Sort::Popular, 10);
		if !popular.is_empty() {
			components.push(HomeComponent {
				title: Some("Popular".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: popular.into_iter().map(Manga::from).collect(),
					auto_scroll_interval: Some(6.0),
				},
			});
		}

		let top_rated = take(Sort::TopRated, 20);
		if !top_rated.is_empty() {
			components.push(HomeComponent {
				title: Some("Top Rated".into()),
				subtitle: None,
				value: HomeComponentValue::MangaList {
					ranking: true,
					page_size: Some(10),
					entries: top_rated.into_iter().map(Link::from).collect(),
					listing: Some(Listing {
						id: "top_rated".into(),
						name: "Top Rated".into(),
						..Default::default()
					}),
				},
			});
		}

		// Newest chapter releases across the whole catalogue.
		if let Ok(posts) = get_json::<Vec<ChapterPost>>(&format!(
			"{CHAPTER_API}/wp-json/wp/v2/posts?per_page={LATEST_PAGE_SIZE}&page=1"
		)) {
			let entries: Vec<MangaWithChapter> = posts
				.iter()
				.filter_map(|post| {
					let chapter = build_chapter(post)?;
					let code = number_of(post.acf.as_ref()?.novel_code.as_ref())? as i64;
					let novel = novels.iter().find(|n| n.code == code)?;
					Some(MangaWithChapter {
						manga: Manga::from(novel.clone()),
						chapter,
					})
				})
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("New Updates".into()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: None,
						entries,
						listing: Some(Listing {
							id: "latest".into(),
							name: "New Updates".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		for (title, id, sort) in [
			("Most Reviewed", "most_reviewed", Sort::MostReviewed),
			("New Arrivals", "new_arrivals", Sort::NewArrivals),
		] {
			let entries: Vec<Link> = take(sort, 20).into_iter().map(Link::from).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some(title.into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: Some(Listing {
							id: id.into(),
							name: title.into(),
							..Default::default()
						}),
					},
				});
			}
		}

		let completed: Vec<Link> = {
			let mut items: Vec<Novel> = novels
				.iter()
				.filter(|n| n.status_matches("completed"))
				.cloned()
				.collect();
			Sort::Popular.apply(&mut items);
			items.into_iter().take(20).map(Link::from).collect()
		};
		if !completed.is_empty() {
			components.push(HomeComponent {
				title: Some("Completed".into()),
				subtitle: None,
				value: HomeComponentValue::Scroller {
					entries: completed,
					listing: Some(Listing {
						id: "completed".into(),
						name: "Completed".into(),
						..Default::default()
					}),
				},
			});
		}

		let genre_items: Vec<FilterItem> = GENRES
			.iter()
			.map(|(id, title)| FilterItem {
				title: (*title).into(),
				values: Some(vec![FilterValue::MultiSelect {
					id: "genres".into(),
					included: vec![(*id).into()],
					excluded: Vec::new(),
				}]),
			})
			.collect();
		components.push(HomeComponent {
			title: Some("Genres".into()),
			subtitle: None,
			value: HomeComponentValue::Filters(genre_items),
		});

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for Mvlempyr {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		if listing.id == "latest" {
			let posts: Vec<ChapterPost> = get_json(&format!(
				"{CHAPTER_API}/wp-json/wp/v2/posts?per_page={LATEST_PAGE_SIZE}&page={}",
				page.max(1)
			))?;
			let novels = catalogue()?;
			let mut entries: Vec<Manga> = Vec::new();
			for post in &posts {
				let Some(code) = post
					.acf
					.as_ref()
					.and_then(|acf| number_of(acf.novel_code.as_ref()))
				else {
					continue;
				};
				let code = code as i64;
				if entries.iter().any(|m| m.key == code.to_string()) {
					continue;
				}
				if let Some(novel) = novels.iter().find(|n| n.code == code) {
					entries.push(Manga::from(novel.clone()));
				}
			}
			return Ok(MangaPageResult {
				has_next_page: posts.len() as i32 == LATEST_PAGE_SIZE,
				entries,
			});
		}

		if listing.id == "completed" {
			let mut novels: Vec<Novel> = catalogue()?
				.into_iter()
				.filter(|n| n.status_matches("completed"))
				.collect();
			Sort::Popular.apply(&mut novels);
			return Ok(page_of(novels, page));
		}

		let sort = match listing.id.as_str() {
			"popular" => Sort::Popular,
			"top_rated" => Sort::TopRated,
			"most_reviewed" => Sort::MostReviewed,
			"new_arrivals" => Sort::NewArrivals,
			_ => bail!("Unknown listing"),
		};
		Ok(page_of(sorted_catalogue(sort)?, page))
	}
}

impl DeepLinkHandler for Mvlempyr {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/novel/") else {
			return Ok(None);
		};
		let slug = url[idx + "/novel/".len()..]
			.split(['/', '?', '#'])
			.next()
			.unwrap_or("");
		if slug.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga {
			key: slug.to_string(),
		}))
	}
}

register_source!(Mvlempyr, Home, ListingProvider, DeepLinkHandler);
