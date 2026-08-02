#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home, HomeComponent,
	HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, MangaWithChapter, Page, PageContent, PageContext, Result, Source,
	Viewer,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::html::{Document, Element},
	imports::net::Request,
	imports::std::{current_date, parse_date},
	prelude::*,
};

const BASE_URL: &str = "https://www.mangatown.com";

const ADULT_GENRES: &[&str] = &["adult", "hentai", "lolicon", "shotacon"];
const MATURE_GENRES: &[&str] = &["ecchi", "mature", "smut", "yaoi", "yuri"];

fn sort_token(index: i32) -> &'static str {
	match index {
		1 => "name.az",
		2 => "rating.za",
		3 => "last_chapter_time.za",
		_ => "",
	}
}

fn abs_url(value: &str) -> String {
	let value = value.trim();
	if value.is_empty() {
		String::new()
	} else if let Some(rest) = value.strip_prefix("//") {
		format!("https://{rest}")
	} else if value.starts_with("http") {
		value.to_string()
	} else if value.starts_with('/') {
		format!("{BASE_URL}{value}")
	} else {
		format!("{BASE_URL}/{value}")
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
	(!id.is_empty()).then(|| id.to_string())
}

fn chapter_ref_from(href: &str) -> Option<(String, String)> {
	// /manga/{id}/((?:v.../)?c.../)
	let idx = href.find("/manga/")?;
	let after = &href[idx + "/manga/".len()..];
	let mut segments = after.split('/').filter(|s| !s.is_empty());
	let manga_id = segments.next()?.to_string();
	let mut chapter = segments.next()?.to_string();
	if chapter.starts_with('v')
		&& let Some(next) = segments.next()
	{
		chapter = format!("{chapter}/{next}");
	}
	if chapter.starts_with('c') || chapter.contains("/c") {
		Some((manga_id, chapter))
	} else {
		None
	}
}

fn chapter_number_from(text: &str) -> Option<f32> {
	let trimmed = text.trim();
	let digits: String = trimmed
		.chars()
		.rev()
		.take_while(|c| c.is_ascii_digit() || *c == '.')
		.collect::<String>()
		.chars()
		.rev()
		.collect();
	digits.trim_matches('.').parse::<f32>().ok()
}

fn content_rating_for(genres: &[String]) -> ContentRating {
	let lowered: Vec<String> = genres.iter().map(|g| g.to_lowercase()).collect();
	if lowered.iter().any(|g| ADULT_GENRES.contains(&g.as_str())) {
		ContentRating::NSFW
	} else if lowered.iter().any(|g| MATURE_GENRES.contains(&g.as_str())) {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

fn parse_site_date(text: &str) -> Option<i64> {
	let lowered = text.trim().to_lowercase();
	if lowered.is_empty() {
		return None;
	}
	if lowered.contains("today") {
		return Some(current_date());
	}
	if lowered.contains("yesterday") {
		return Some(current_date() - 86400);
	}

	// Cards often write the age instead of a date, with no `ago` and a
	// shortened unit, so match the unit on its stem.
	let mut words = lowered.trim_end_matches("ago").split_whitespace();
	if let Some(amount) = words.next().and_then(|word| word.parse::<i64>().ok())
		&& let Some(unit) = words.next()
	{
		let seconds = if unit.starts_with("second") {
			Some(1)
		} else if unit.starts_with("min") {
			Some(60)
		} else if unit.starts_with("hour") || unit.starts_with("hr") {
			Some(3600)
		} else if unit.starts_with("day") {
			Some(86400)
		} else if unit.starts_with("week") {
			Some(604800)
		} else if unit.starts_with("month") {
			Some(2592000)
		} else if unit.starts_with("year") {
			Some(31536000)
		} else {
			None
		};
		if let Some(seconds) = seconds {
			return Some(current_date() - amount * seconds);
		}
	}

	parse_date(text.trim(), "MMM d, yyyy")
}

struct ListItem {
	manga: Manga,
	chapter_key: Option<String>,
	chapter_number: Option<f32>,
}

fn parse_list(doc: &Document) -> Vec<ListItem> {
	let mut items = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	let Some(lis) = doc.select("li") else {
		return items;
	};
	for li in lis {
		let Some(cover) = li.select_first("a.manga_cover") else {
			continue;
		};
		let link = li
			.select_first("p.title a")
			.or_else(|| li.select_first("p a"));
		let href = link
			.as_ref()
			.and_then(|a| a.attr("href"))
			.or_else(|| cover.attr("href"))
			.unwrap_or_default();
		let Some(manga_id) = manga_id_from(&href) else {
			continue;
		};
		let title = link
			.as_ref()
			.and_then(|a| a.text())
			.or_else(|| cover.attr("title"))
			.unwrap_or_default();
		let title = title.trim().to_string();
		if title.is_empty() || seen.iter().any(|s| s == &manga_id) {
			continue;
		}
		seen.push(manga_id.clone());

		let genres: Vec<String> = li
			.select("p.keyWord a")
			.map(|els| {
				els.filter_map(|a| a.text())
					.map(|t| t.trim().to_string())
					.filter(|t| !t.is_empty())
					.collect()
			})
			.unwrap_or_default();

		let cover_url = li
			.select_first("a.manga_cover img")
			.map(|img| img_from(&img));

		let chapter = li.select_first("p.new_chapter a");
		let (chapter_key, chapter_number) = match chapter {
			Some(a) => {
				let href = a.attr("href").unwrap_or_default();
				let key = chapter_ref_from(&href).map(|(_, c)| c);
				let num = a.text().and_then(|t| chapter_number_from(&t));
				(key, num)
			}
			None => (None, None),
		};

		let content_rating = content_rating_for(&genres);
		items.push(ListItem {
			manga: Manga {
				key: manga_id,
				title,
				cover: cover_url,
				tags: (!genres.is_empty()).then_some(genres),
				content_rating,
				..Default::default()
			},
			chapter_key,
			chapter_number,
		});
	}
	items
}

fn has_next_page(doc: &Document) -> bool {
	doc.select("a.next")
		.map(|els| {
			els.into_iter()
				.any(|a| !a.attr("href").unwrap_or_default().starts_with("javascript"))
		})
		.unwrap_or(false)
}

fn fetch_list(url: &str) -> Result<MangaPageResult> {
	let doc = Request::get(url)?.html()?;
	let has_next_page = has_next_page(&doc);
	let entries = parse_list(&doc).into_iter().map(|i| i.manga).collect();
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

fn directory_url(page: i32, demographic: &str, genre: &str, status: &str, sort: &str) -> String {
	let mut url = format!(
		"{BASE_URL}/directory/{}-{}-0-{}-0-0/{}.htm",
		if demographic.is_empty() {
			"0"
		} else {
			demographic
		},
		if genre.is_empty() { "0" } else { genre },
		if status.is_empty() { "0" } else { status },
		page.max(1),
	);
	if !sort.is_empty() {
		url.push('?');
		url.push_str(sort);
	}
	url
}

fn hot_url(page: i32, demographic: &str, sort: &str) -> String {
	let mut url = if demographic.is_empty() {
		format!("{BASE_URL}/hot/{}.htm", page.max(1))
	} else {
		format!("{BASE_URL}/hot/{demographic}/{}.htm", page.max(1))
	};
	if !sort.is_empty() {
		url.push('?');
		url.push_str(sort);
	}
	url
}

fn links_from(url: &str) -> Vec<Link> {
	fetch_list(url)
		.map(|result| {
			result
				.entries
				.into_iter()
				.map(|manga| Link {
					title: manga.title.clone(),
					subtitle: manga.tags.as_ref().map(|tags| tags.join(" · ")),
					image_url: manga.cover.clone(),
					value: Some(LinkValue::Manga(manga)),
				})
				.collect()
		})
		.unwrap_or_default()
}

struct MangaTown;

impl Source for MangaTown {
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

		let mut sort = 0;
		let mut demographic = String::new();
		let mut completed = String::new();
		let mut author = String::new();
		let mut artist = String::new();
		let mut included: Vec<String> = Vec::new();
		let mut excluded: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => sort = index,
				FilterValue::Select { id, value } if id == "demographic" => demographic = value,
				FilterValue::Select { id, value } if id == "completed" => completed = value,
				FilterValue::Text { id, value } if id == "author" => author = value,
				FilterValue::Text { id, value } if id == "artist" => artist = value,
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

		let use_search = !query.is_empty()
			|| !included.is_empty()
			|| !excluded.is_empty()
			|| !demographic.is_empty()
			|| !completed.is_empty()
			|| !author.is_empty()
			|| !artist.is_empty();

		if use_search {
			let mut qs = QueryParameters::new();
			qs.push("page", Some(&page.to_string()));
			if !query.is_empty() {
				qs.push("name", Some(query));
			}
			if !author.is_empty() {
				qs.push("author", Some(&author));
			}
			if !artist.is_empty() {
				qs.push("artist", Some(&artist));
			}
			for genre in &included {
				qs.push(&format!("genres[{genre}]"), Some("1"));
			}
			for genre in &excluded {
				qs.push(&format!("genres[{genre}]"), Some("2"));
			}
			if !demographic.is_empty() {
				qs.push(&format!("genres[{demographic}]"), Some("1"));
			}
			if !completed.is_empty() {
				qs.push("is_completed", Some(&completed));
			}
			fetch_list(&format!("{BASE_URL}/search?{qs}"))
		} else {
			let token = sort_token(sort);
			let suffix = if token.is_empty() {
				String::new()
			} else {
				format!("?{token}")
			};
			fetch_list(&format!(
				"{BASE_URL}/directory/0-0-0-0-0-0/{page}.htm{suffix}"
			))
		}
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{BASE_URL}/manga/{}/", manga.key);
		let doc = Request::get(&url)?.html()?;

		if needs_details {
			let info = doc.select_first("div.article_content");
			manga.title = info
				.as_ref()
				.and_then(|i| i.select_first("h1"))
				.and_then(|h| h.text())
				.map(|t| t.trim().to_string())
				.filter(|t| !t.is_empty())
				.unwrap_or_else(|| manga.key.clone());
			manga.cover = doc
				.select_first("div.detail_info img")
				.map(|img| img_from(&img));

			let mut genres: Vec<String> = Vec::new();
			let mut status = MangaStatus::Unknown;
			let mut author: Option<String> = None;
			let mut artist: Option<String> = None;
			if let Some(info) = info.as_ref()
				&& let Some(lis) = info.select("li")
			{
				for li in lis {
					let label = li
						.select_first("b")
						.and_then(|b| b.text())
						.unwrap_or_default()
						.to_lowercase();
					let li_text = li.text().unwrap_or_default();
					let value = li_text
						.split_once(':')
						.map(|(_, v)| v.trim().to_string())
						.unwrap_or_default();
					if label.contains("genre") {
						genres = li
							.select("a")
							.map(|els| {
								els.filter_map(|a| a.text())
									.map(|t| t.trim().to_string())
									.filter(|t| !t.is_empty())
									.collect()
							})
							.unwrap_or_default();
					} else if label.contains("status") {
						let lower = value.to_lowercase();
						status = if lower.contains("ongoing") {
							MangaStatus::Ongoing
						} else if lower.contains("completed") {
							MangaStatus::Completed
						} else {
							MangaStatus::Unknown
						};
					} else if label.contains("author") && !value.is_empty() {
						author = Some(value);
					} else if label.contains("artist") && !value.is_empty() {
						artist = Some(value);
					}
				}
			}

			manga.description = doc
				.select_first("span#show")
				.and_then(|el| el.text())
				.map(|t| t.trim().trim_end_matches("HIDE").trim().to_string())
				.filter(|t| !t.is_empty());
			manga.status = status;
			manga.content_rating = content_rating_for(&genres);
			manga.authors = author.map(|a| vec![a]);
			manga.artists = artist.map(|a| vec![a]);
			manga.tags = (!genres.is_empty()).then_some(genres);
			manga.viewer = Viewer::RightToLeft;
			manga.url = Some(url.clone());

			if needs_chapters {
				aidoku::imports::std::send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();
			let mut seen: Vec<String> = Vec::new();
			if let Some(lis) = doc.select("ul.chapter_list li") {
				for li in lis {
					let Some(link) = li.select_first("a") else {
						continue;
					};
					let href = link.attr("href").unwrap_or_default();
					let Some((_, chapter_key)) = chapter_ref_from(&href) else {
						continue;
					};
					if seen.iter().any(|s| s == &chapter_key) {
						continue;
					}
					seen.push(chapter_key.clone());
					let link_text = link.text().unwrap_or_default();
					let chapter_number = chapter_number_from(&link_text);
					let volume_number = chapter_key
						.strip_prefix('v')
						.and_then(|rest| rest.split('/').next())
						.and_then(|v| v.parse::<f32>().ok());
					let date_uploaded = li
						.select_first("span.time")
						.and_then(|el| el.text())
						.and_then(|t| parse_site_date(&t));
					chapters.push(Chapter {
						url: Some(format!("{BASE_URL}/manga/{}/{}/", manga.key, chapter_key)),
						key: chapter_key,
						chapter_number,
						volume_number,
						date_uploaded,
						language: Some("en".into()),
						..Default::default()
					});
				}
			}
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{BASE_URL}/manga/{}/{}/", manga.key, chapter.key);
		let doc = Request::get(&url)?.html()?;

		let page_urls = parse_page_option_urls(&doc);
		if !page_urls.is_empty() {
			let first_img = doc
				.select_first("div#viewer img, img#image, source#image")
				.map(|img| img_from(&img))
				.unwrap_or_default();
			if let Some(urls) = build_sequential(&first_img, page_urls.len()) {
				return Ok(urls
					.into_iter()
					.map(|u| Page {
						content: PageContent::url(u),
						..Default::default()
					})
					.collect());
			}
			// Fallback: fetch each page and read its image.
			let mut pages = Vec::new();
			for page_url in page_urls {
				if let Ok(page_doc) = Request::get(&page_url)?.html() {
					let img = page_doc
						.select_first("div#viewer img, img#image, source#image")
						.map(|img| img_from(&img))
						.unwrap_or_default();
					if !img.is_empty() {
						pages.push(Page {
							content: PageContent::url(img),
							..Default::default()
						});
					}
				}
			}
			if pages.is_empty() {
				bail!("No pages found");
			}
			return Ok(pages);
		}

		// Long-strip: all images on one page.
		let pages: Vec<Page> = doc
			.select("div#viewer img")
			.map(|els| {
				els.filter_map(|el| {
					let url = img_from(&el);
					(!url.is_empty()).then_some(Page {
						content: PageContent::url(url),
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();
		if pages.is_empty() {
			bail!("No pages found");
		}
		Ok(pages)
	}
}

fn parse_page_option_urls(doc: &Document) -> Vec<String> {
	let options = doc
		.select("select#top_chapter_list ~ div.page_select option")
		.filter(|els| !els.is_empty())
		.or_else(|| doc.select("div.manga_read_footer div.page_select option"))
		.or_else(|| doc.select("div.page_select option"));
	let mut urls = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	if let Some(options) = options {
		for option in options {
			let value = option.attr("value").unwrap_or_default();
			let text = option.text().unwrap_or_default();
			if value.is_empty() || value.contains("featured") || text.contains("Featured") {
				continue;
			}
			let url = abs_url(&value);
			if seen.iter().any(|s| s == &url) {
				continue;
			}
			seen.push(url.clone());
			urls.push(url);
		}
	}
	urls
}

/// Derives every page image url from the first one when they are numbered
/// sequentially in the same directory.
fn build_sequential(first_img: &str, total: usize) -> Option<Vec<String>> {
	if first_img.is_empty() || total == 0 {
		return None;
	}
	let slash = first_img.rfind('/')?;
	let (dir, filename) = first_img.split_at(slash + 1);
	let dot = filename.rfind('.')?;
	let (stem, ext) = filename.split_at(dot);
	let digits: String = stem
		.chars()
		.rev()
		.take_while(|c| c.is_ascii_digit())
		.collect::<String>()
		.chars()
		.rev()
		.collect();
	if digits.is_empty() {
		return None;
	}
	let prefix = &stem[..stem.len() - digits.len()];
	if prefix.chars().any(|c| c.is_ascii_digit()) {
		return None;
	}
	let width = digits.len();
	let first: i64 = digits.parse().ok()?;
	Some(
		(0..total as i64)
			.map(|i| format!("{dir}{prefix}{:0width$}{ext}", first + i))
			.collect(),
	)
}

impl Home for MangaTown {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components: Vec<HomeComponent> = Vec::new();

		if let Ok(doc) = Request::get(format!("{BASE_URL}/featured/"))?.html() {
			let entries: Vec<Manga> = parse_list(&doc)
				.into_iter()
				.take(8)
				.map(|item| {
					let manga = item.manga;
					self.get_manga_update(manga.clone(), true, false)
						.unwrap_or(manga)
				})
				.collect();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: Some("Featured Manga".into()),
					subtitle: None,
					value: HomeComponentValue::BigScroller {
						entries,
						auto_scroll_interval: Some(5.0),
					},
				});
			}
		}

		let hot = links_from(&hot_url(1, "", ""));
		if !hot.is_empty() {
			components.push(HomeComponent {
				title: Some("Hot Manga".into()),
				subtitle: None,
				value: HomeComponentValue::MangaList {
					ranking: true,
					page_size: Some(10),
					entries: hot,
					listing: Some(Listing {
						id: "hot_total".into(),
						name: "Hot Manga".into(),
						..Default::default()
					}),
				},
			});
		}

		if let Ok(doc) = Request::get(directory_url(1, "", "", "", "last_chapter_time.za"))?.html()
		{
			let entries: Vec<MangaWithChapter> = parse_list(&doc)
				.into_iter()
				.filter_map(|item| {
					let key = item.chapter_key?;
					Some(MangaWithChapter {
						manga: item.manga,
						chapter: Chapter {
							key,
							chapter_number: item.chapter_number,
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

		for (title, id, demographic, genre, status, sort) in [
			("New Manga Releases", "new", "", "", "new", ""),
			(
				"Romance Releases",
				"romance",
				"",
				"romance",
				"",
				"last_chapter_time.za",
			),
			(
				"Shounen Releases",
				"shounen",
				"shounen",
				"",
				"",
				"last_chapter_time.za",
			),
			(
				"Seinen Releases",
				"seinen",
				"seinen",
				"",
				"",
				"last_chapter_time.za",
			),
			(
				"Shoujo Releases",
				"shoujo",
				"shoujo",
				"",
				"",
				"last_chapter_time.za",
			),
			("Josei Releases", "josei", "josei", "", "", ""),
			("Yaoi Releases", "yaoi", "yaoi", "", "", ""),
			(
				"Shounen Ai Releases",
				"shounen_ai",
				"shounen_ai",
				"",
				"",
				"",
			),
		] {
			let entries = links_from(&directory_url(1, demographic, genre, status, sort));
			if entries.is_empty() {
				continue;
			}
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

			let top = match demographic {
				"shounen" => Some(("Top Shounen", "top_shounen")),
				"seinen" => Some(("Top Seinen", "top_seinen")),
				"shoujo" => Some(("Top Shoujo", "top_shoujo")),
				"yaoi" => Some(("Top Yaoi", "top_yaoi")),
				_ => None,
			};
			if let Some((top_title, top_id)) = top {
				let entries = links_from(&hot_url(1, demographic, ""));
				if !entries.is_empty() {
					components.push(HomeComponent {
						title: Some(top_title.into()),
						subtitle: None,
						value: HomeComponentValue::MangaList {
							ranking: true,
							page_size: Some(10),
							entries,
							listing: Some(Listing {
								id: top_id.into(),
								name: top_title.into(),
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

impl ListingProvider for MangaTown {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let page = page.max(1);
		let url = match listing.id.as_str() {
			"views" => directory_url(page, "", "", "", ""),
			"latest" => directory_url(page, "", "", "", "last_chapter_time.za"),
			"rating" => directory_url(page, "", "", "", "rating.za"),
			"new" => directory_url(page, "", "", "new", ""),
			"romance" => directory_url(page, "", "romance", "", "last_chapter_time.za"),
			"shounen" => directory_url(page, "shounen", "", "", "last_chapter_time.za"),
			"seinen" => directory_url(page, "seinen", "", "", "last_chapter_time.za"),
			"shoujo" => directory_url(page, "shoujo", "", "", "last_chapter_time.za"),
			"josei" => directory_url(page, "josei", "", "", ""),
			"yaoi" => directory_url(page, "yaoi", "", "", ""),
			"shounen_ai" => directory_url(page, "shounen_ai", "", "", ""),
			"hot_total" => hot_url(page, "", ""),
			"hot_month" => hot_url(page, "", "mviews.za"),
			"hot_week" => hot_url(page, "", "wviews.za"),
			"hot_today" => hot_url(page, "", "tviews.za"),
			"top_shounen" => hot_url(page, "shounen", ""),
			"top_seinen" => hot_url(page, "seinen", ""),
			"top_shoujo" => hot_url(page, "shoujo", ""),
			"top_yaoi" => hot_url(page, "yaoi", ""),
			_ => bail!("Unknown listing"),
		};
		fetch_list(&url)
	}
}

impl aidoku::ImageRequestProvider for MangaTown {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
	}
}

impl DeepLinkHandler for MangaTown {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(manga_id) = manga_id_from(&url) else {
			return Ok(None);
		};
		if let Some((_, chapter_key)) = chapter_ref_from(&url) {
			return Ok(Some(DeepLinkResult::Chapter {
				manga_key: manga_id,
				key: chapter_key,
			}));
		}
		Ok(Some(DeepLinkResult::Manga { key: manga_id }))
	}
}

register_source!(
	MangaTown,
	Home,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
