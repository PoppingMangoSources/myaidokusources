#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::net::Request,
	imports::std::send_partial_result,
	prelude::*,
};
use serde_json::Value;

mod models;
mod network;
mod parsers;
mod settings;
mod webview;

use models::*;
use network::make_request;
use parsers::*;

struct Mkissa;

fn source_content_rating() -> ContentRating {
	if settings::show_adult() {
		ContentRating::NSFW
	} else {
		ContentRating::Suggestive
	}
}

fn json_opt_string(value: Option<String>) -> Value {
	match value {
		Some(s) => Value::String(s),
		None => Value::Null,
	}
}

fn json_string_array(values: Vec<String>) -> Value {
	if values.is_empty() {
		Value::Null
	} else {
		Value::Array(values.into_iter().map(Value::String).collect())
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

/// A one-line blurb from the score and view count the cards already carry.
fn card_summary(score: Option<f32>, views: Option<i64>) -> Option<String> {
	let mut parts: Vec<String> = Vec::new();
	if let Some(score) = score.filter(|score| *score > 0.0) {
		parts.push(format!("★ {score:.1}"));
	}
	if let Some(views) = views.filter(|views| *views > 0) {
		parts.push(format!("{} views", format_count(views)));
	}
	(!parts.is_empty()).then(|| parts.join(" · "))
}

fn card_to_manga(card: MangaCard) -> Manga {
	let title = decode_entities(card.display_title());
	let cover = parse_thumbnail_url(card.thumbnail.as_deref());
	let description = card_summary(card.score, None);
	Manga {
		key: card.id,
		title,
		cover: Some(cover),
		description,
		content_rating: source_content_rating(),
		..Default::default()
	}
}

/// Same as `card_to_manga`, but keeps the view count the recommendation adds.
fn recommendation_to_manga(rec: Recommendation) -> Option<Manga> {
	let views = rec.page_status.as_ref().and_then(|status| status.views);
	let card = rec.any_card?;
	let score = card.score;
	let mut manga = card_to_manga(card);
	manga.description = card_summary(score, views);
	Some(manga)
}

fn card_to_link(card: MangaCard) -> Link {
	let manga = card_to_manga(card);
	Link {
		title: manga.title.clone(),
		subtitle: manga.description.clone(),
		image_url: manga.cover.clone(),
		value: Some(LinkValue::Manga(manga)),
	}
}

fn popular_cards(date_range: i32, page: i32) -> Result<(Vec<Recommendation>, bool)> {
	let variables = serde_json::json!({
		"type": "manga",
		"size": LIMIT,
		"dateRange": date_range,
		"page": page,
		"allowAdult": settings::show_adult(),
		"allowUnknown": false,
	});
	let data: PopularData = make_request(POPULAR_QUERY, variables)?;
	let recommendations = data.query_popular.recommendations;
	let has_next = recommendations.len() as i32 == LIMIT;
	Ok((recommendations, has_next))
}

fn popular_listing(date_range: i32, page: i32) -> Result<MangaPageResult> {
	let (recommendations, has_next_page) = popular_cards(date_range, page)?;
	let entries = recommendations
		.into_iter()
		.filter_map(recommendation_to_manga)
		.collect();
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

fn latest_data(page: i32) -> Result<SearchData> {
	let variables = serde_json::json!({
		"search": {
			"query": Value::Null,
			"sortBy": Value::Null,
			"genres": Value::Null,
			"excludeGenres": Value::Null,
			"isManga": true,
			"allowAdult": settings::show_adult(),
			"allowUnknown": false,
		},
		"size": LIMIT,
		"page": page,
		"translationType": "sub",
		"countryOrigin": "ALL",
	});
	make_request(LATEST_QUERY, variables)
}

impl Source for Mkissa {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let raw = query.unwrap_or_default();
		let title = raw.trim();

		if let Some(result) = self.resolve_direct_query(title)? {
			return Ok(result);
		}

		let page = page.max(1);
		let mut sort_id: Option<String> = None;
		let mut country = String::from("ALL");
		let mut included: Vec<String> = Vec::new();
		let mut excluded: Vec<String> = Vec::new();

		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					sort_id = match index {
						1 => Some("Name_ASC".into()),
						2 => Some("Name_DESC".into()),
						_ => None,
					};
				}
				FilterValue::Select { id, value } if id == "country" => {
					country = value;
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

		let search_input = serde_json::json!({
			"query": json_opt_string(if title.is_empty() { None } else { Some(title.to_string()) }),
			"sortBy": json_opt_string(sort_id),
			"genres": json_string_array(included),
			"excludeGenres": json_string_array(excluded),
			"isManga": true,
			"allowAdult": settings::show_adult(),
			"allowUnknown": false,
		});
		let variables = serde_json::json!({
			"search": search_input,
			"size": LIMIT,
			"page": page,
			"translationType": "sub",
			"countryOrigin": country,
		});

		let data: SearchData = make_request(SEARCH_QUERY, variables)?;
		let has_next_page = data.mangas.edges.len() as i32 == LIMIT;
		let entries = data.mangas.edges.into_iter().map(card_to_manga).collect();
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
		let key = manga.key.clone();

		if needs_details {
			let data: DetailsData =
				make_request(DETAILS_QUERY, serde_json::json!({ "id": key.as_str() }))?;
			let detail = data.manga;

			manga.title = decode_entities(match &detail.english_name {
				Some(name) if !name.is_empty() => name,
				_ => &detail.name,
			});
			manga.cover = Some(parse_thumbnail_url(detail.thumbnail.as_deref()));
			if let Some(description) = &detail.description {
				manga.description = Some(strip_html(description));
			}

			let mut genres: Vec<String> = Vec::new();
			for genre in detail
				.genres
				.unwrap_or_default()
				.into_iter()
				.chain(detail.tags.unwrap_or_default())
			{
				let trimmed = genre.trim();
				if !trimmed.is_empty() && !genres.iter().any(|g| g == trimmed) {
					genres.push(trimmed.to_string());
				}
			}
			manga.content_rating = content_rating_for_genres(&genres);
			if !genres.is_empty() {
				manga.tags = Some(genres);
			}

			if let Some(authors) = detail.authors {
				let authors: Vec<String> = authors
					.into_iter()
					.map(|a| a.trim().to_string())
					.filter(|a| !a.is_empty())
					.collect();
				if !authors.is_empty() {
					manga.artists = Some(authors.clone());
					manga.authors = Some(authors);
				}
			}

			manga.status = parse_status(detail.status.as_deref());
			manga.url = Some(format!("{DOMAIN}/manga/{key}"));

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let variables = serde_json::json!({
				"id": key.as_str(),
				"showId": format!("manga@{key}"),
			});
			let data: ChaptersData = make_request(CHAPTERS_QUERY, variables)?;
			manga.chapters = Some(parse_chapters(&data, &key));
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let urls = webview::page_urls_via_webview(&manga.key, &chapter.key)?;
		Ok(urls
			.into_iter()
			.map(|url| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

impl Mkissa {
	fn resolve_direct_query(&self, query: &str) -> Result<Option<MangaPageResult>> {
		let id = if query.to_lowercase().starts_with("id:") {
			let value = query[3..].trim();
			(!value.is_empty()).then(|| value.to_string())
		} else if let Some(idx) = query.find("/manga/") {
			let after = &query[idx + "/manga/".len()..];
			let segment = after.split(['/', '?', '#']).next().unwrap_or("");
			(!segment.is_empty()).then(|| segment.to_string())
		} else {
			None
		};

		let Some(id) = id else {
			return Ok(None);
		};

		let manga = self.get_manga_update(
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
}

impl Home for Mkissa {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok((recommendations, _)) = popular_cards(0, 1) {
			let entries: Vec<Manga> = recommendations
				.into_iter()
				.filter_map(recommendation_to_manga)
				.collect();
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

		for (title, id, date_range) in [
			("Popular This Week", "popular_week", 7),
			("Popular This Month", "popular_month", 30),
		] {
			if let Ok((recommendations, _)) = popular_cards(date_range, 1) {
				let entries: Vec<Link> = recommendations
					.into_iter()
					.filter_map(recommendation_to_manga)
					.map(Into::into)
					.collect();
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
		}

		if let Ok(data) = latest_data(1) {
			let entries: Vec<MangaWithChapter> = data
				.mangas
				.edges
				.into_iter()
				.filter_map(|card| {
					let latest = card
						.available_chapters_detail
						.as_ref()
						.and_then(|detail| detail.sub.as_ref())
						.and_then(|sub| sub.first())
						.cloned()?;
					let date_uploaded = card
						.last_chapter_date
						.as_ref()
						.and_then(|last| date_from_parts(last.sub.as_ref()));
					let manga = card_to_manga(card);
					Some(MangaWithChapter {
						manga,
						chapter: Chapter {
							key: latest.clone(),
							title: Some(format!("Chapter {latest}")),
							chapter_number: latest.parse::<f32>().ok(),
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
							id: "latest_updates".into(),
							name: "Latest Updates".into(),
							..Default::default()
						}),
					},
				});
			}
		}

		if let Ok(data) = make_request::<RandomData>(
			RANDOM_QUERY,
			serde_json::json!({ "format": "manga", "allowAdult": settings::show_adult() }),
		) {
			let entries: Vec<Link> = data
				.query_random_recommendation
				.unwrap_or_default()
				.into_iter()
				.map(card_to_link)
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Recommended".into()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries,
						listing: None,
					},
				});
			}
		}

		Ok(HomeLayout { components })
	}
}

impl ListingProvider for Mkissa {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let page = page.max(1);
		match listing.id.as_str() {
			"popular_today" => popular_listing(1, page),
			"popular_week" => popular_listing(7, page),
			"popular_month" => popular_listing(30, page),
			"popular_all_time" => popular_listing(0, page),
			"latest_updates" => {
				let data = latest_data(page)?;
				let has_next_page = data.mangas.edges.len() as i32 == LIMIT;
				let entries = data.mangas.edges.into_iter().map(card_to_manga).collect();
				Ok(MangaPageResult {
					entries,
					has_next_page,
				})
			}
			_ => bail!("Unknown listing"),
		}
	}
}

impl aidoku::ImageRequestProvider for Mkissa {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{DOMAIN}/"))
			.header("Origin", DOMAIN))
	}
}

impl DeepLinkHandler for Mkissa {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(idx) = url.find("/manga/") else {
			return Ok(None);
		};
		let after = &url[idx + "/manga/".len()..];
		let mut segments = after.split('/');
		let Some(manga_key) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};

		if let Some(chapter_segment) = segments.next() {
			// ex: chapter-12-sub -> 12
			if let Some(rest) = chapter_segment.strip_prefix("chapter-") {
				let number = rest.trim_end_matches("-sub");
				if !number.is_empty() {
					return Ok(Some(DeepLinkResult::Chapter {
						manga_key: manga_key.to_string(),
						key: number.to_string(),
					}));
				}
			}
		}

		Ok(Some(DeepLinkResult::Manga {
			key: manga_key.to_string(),
		}))
	}
}

register_source!(
	Mkissa,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
