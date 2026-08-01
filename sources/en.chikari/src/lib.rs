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

fn api_get<T: DeserializeOwned>(url: &str) -> Result<T> {
	Request::get(url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Origin", DOMAIN)
		.header("Accept", "application/json, text/plain, */*")
		.send()?
		.get_json_owned()
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
	let content_rating = if item.is_nsfw {
		ContentRating::NSFW
	} else {
		ContentRating::Safe
	};
	Manga {
		key: item.slug,
		title: item.title,
		cover: Some(cover),
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
		subtitle: None,
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
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
	query: Option<&str>,
	types: &[String],
	statuses: &[String],
	offset: i32,
) -> Result<SeriesListResponse> {
	let mut qs = QueryParameters::new();
	qs.push("sort", Some(sort));
	qs.push("adult", Some(&settings::adult().to_string()));
	qs.push(
		"content_rating",
		Some(&settings::content_ratings().join(",")),
	);
	qs.push("limit", Some(&PAGE_SIZE.to_string()));
	qs.push("offset", Some(&offset.to_string()));
	if let Some(query) = query.filter(|q| !q.is_empty()) {
		qs.push("q", Some(query));
	}
	let types = if types.is_empty() {
		settings::content_types()
	} else {
		types.to_vec()
	};
	qs.push("type", Some(&types.join(",")));
	if !statuses.is_empty() {
		qs.push("status", Some(&statuses.join(",")));
	}
	api_get(&format!("{API_URL}/series?{qs}"))
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

		let mut sort = "popular";
		let mut types: Vec<String> = Vec::new();
		let mut statuses: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = sort_id(index),
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
			sort,
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
		qs.push("type", Some(&settings::content_types().join(",")));
		let home: HomeResponse = api_get(&format!("{API_URL}/home?{qs}"))?;

		let mut components: Vec<HomeComponent> = Vec::new();
		for row in home.rows {
			if row.items.is_empty() {
				continue;
			}
			match row.slug.as_str() {
				"popular" => {
					let entries = row.items.into_iter().map(item_to_manga).collect();
					components.push(HomeComponent {
						title: Some("Popular".into()),
						subtitle: None,
						value: HomeComponentValue::BigScroller {
							entries,
							auto_scroll_interval: Some(5.0),
						},
					});
				}
				"recently-updated" => {
					let entries: Vec<MangaWithChapter> = row
						.items
						.into_iter()
						.filter_map(|item| {
							let latest = item.latest_chapter?;
							let date_uploaded =
								item.last_chapter_at.as_deref().and_then(parse_iso_date);
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
					components.push(HomeComponent {
						title: Some("Recently Updated".into()),
						subtitle: None,
						value: HomeComponentValue::MangaChapterList {
							page_size: None,
							entries,
							listing: Some(Listing {
								id: "updated".into(),
								name: "Recently Updated".into(),
								..Default::default()
							}),
						},
					});
				}
				slug => {
					let (title, listing_id) = match slug {
						"trending" => ("Trending", "trending"),
						"top-rated" => ("Top Rated", "top_rated"),
						"recently-added" => ("Recently Added", "added"),
						_ => continue,
					};
					let entries: Vec<Link> = row.items.into_iter().map(item_to_link).collect();
					let listing = Some(Listing {
						id: listing_id.into(),
						name: title.into(),
						..Default::default()
					});
					// Top Rated reads as a chart, so show it ranked.
					let value = if slug == "top-rated" {
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

		// The home feed has no bookmark row, so pull it from the series endpoint.
		if let Ok(data) = fetch_series("most_bookmarked", None, &[], &[], 0) {
			let entries: Vec<Link> = data.items.into_iter().map(item_to_link).collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Most Bookmarked".into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: Some(Listing {
							id: "most_bookmarked".into(),
							name: "Most Bookmarked".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for Chikari {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let sort = match listing.id.as_str() {
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
		let data = fetch_series(sort, None, &[], &[], offset)?;
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
