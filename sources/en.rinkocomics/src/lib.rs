#![no_std]
use aidoku::{
	Chapter, ContentRating, HomeComponent, HomeComponentValue, HomeLayout, Manga, MangaWithChapter,
	Result, Source,
	alloc::{String, Vec, string::ToString},
	helpers::string::StripPrefixOrSelf,
	imports::{
		html::{Element, Html},
		net::Request,
	},
	prelude::*,
};
use madara::{Impl, Madara, Params};

mod chapters;

const BASE_URL: &str = "https://rinkocomics.com";

struct RinkoComics;

fn image_from(element: &Element, selector: &str) -> Option<String> {
	let image = element.select_first(selector)?;
	image
		.attr("abs:data-src")
		.or_else(|| image.attr("abs:data-lazy-src"))
		.or_else(|| image.attr("abs:src"))
		.or_else(|| image.attr("src"))
}

fn card_manga(
	params: &Params,
	element: &Element,
	link_selector: &str,
	title_selector: &str,
	image_selector: &str,
) -> Option<Manga> {
	let href = if link_selector.is_empty() {
		element.attr("abs:href").or_else(|| element.attr("href"))?
	} else {
		element
			.select_first(link_selector)?
			.attr("abs:href")
			.or_else(|| element.select_first(link_selector)?.attr("href"))?
	};
	let title = element
		.select_first(title_selector)
		.and_then(|el| el.text())
		.or_else(|| {
			element
				.select_first(link_selector)
				.and_then(|el| el.attr("title"))
		})?;
	// Every card variant on the home page marks its genres and latest chapter
	// the same way, so pick them up wherever they happen to be present.
	let tags: Vec<String> = element
		.select(
			".comic-genres .genre, .comic-genres-popular span, .novel-genres .genre-tag, .genre",
		)
		.map(|genres| {
			genres
				.filter_map(|genre| genre.text())
				.map(|genre| genre.trim().to_string())
				.filter(|genre| !genre.is_empty())
				.collect()
		})
		.unwrap_or_default();
	let description = element
		.select_first(".chapter-badge")
		.and_then(|badge| badge.text())
		.map(|text| text.trim().to_string())
		.filter(|text| !text.is_empty());

	Some(Manga {
		key: href.strip_prefix_or_self(&params.base_url).into(),
		title: title.trim().to_string(),
		cover: image_from(element, image_selector),
		description,
		content_rating: ContentRating::Safe,
		tags: (!tags.is_empty()).then_some(tags),
		url: Some(href),
		..Default::default()
	})
}

fn chapter_number(title: &str) -> Option<f32> {
	let mut number = String::new();
	for ch in title.chars() {
		if ch.is_ascii_digit() || (ch == '.' && !number.is_empty()) {
			number.push(ch);
		} else if !number.is_empty() {
			break;
		}
	}
	number.trim_matches('.').parse().ok()
}

impl Impl for RinkoComics {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			source_path: "comic".into(),
			..Default::default()
		}
	}

	/// The site paginates chapters through its own ajax action instead of the
	/// Madara endpoint, so the chapter list is collected here.
	fn get_manga_update(
		&self,
		params: &Params,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{}{}", params.base_url, manga.key);
		let body = Request::get(&url)?
			.header("Referer", &format!("{}/", params.base_url))
			.string()?;
		let html = Html::parse_with_url(&body, &url)?;

		if needs_details {
			self.apply_manga_details(params, &mut manga, &html, &url);
		}

		if needs_chapters {
			manga.chapters = Some(chapters::parse_chapters(&body, &html, &params.base_url)?);
		}

		Ok(manga)
	}

	fn get_home(&self, params: &Params) -> Result<HomeLayout> {
		let html = Request::get(&params.base_url)?.html()?;
		let mut components = Vec::new();

		let featured: Vec<aidoku::Link> = html
			.select(".hero-slider .slide")
			.map(|items| {
				items
					.filter_map(|item| {
						let manga = card_manga(params, &item, "a", ".comic-title", "img")?;
						Some(aidoku::Link {
							title: manga.title.clone(),
							subtitle: manga
								.tags
								.as_ref()
								.filter(|genres| !genres.is_empty())
								.map(|genres| genres.join(" · ")),
							image_url: manga.cover.clone(),
							value: Some(aidoku::LinkValue::Manga(manga)),
						})
					})
					.collect()
			})
			.unwrap_or_default();
		if !featured.is_empty() {
			components.push(HomeComponent {
				title: Some("Featured".into()),
				subtitle: None,
				value: HomeComponentValue::ImageScroller {
					links: featured,
					auto_scroll_interval: Some(6.0),
					width: None,
					height: None,
				},
			});
		}

		let hot: Vec<aidoku::Link> = html
			.select(".popular-comics .comic-card-popular")
			.map(|items| {
				items
					.filter_map(|item| {
						card_manga(
							params,
							&item,
							"a.read-btn",
							".comic-title-popular",
							".comic-cover img",
						)
					})
					.map(Into::into)
					.collect()
			})
			.unwrap_or_default();
		if !hot.is_empty() {
			components.push(HomeComponent {
				title: Some("Hot This Week".into()),
				subtitle: None,
				value: HomeComponentValue::MangaList {
					ranking: true,
					page_size: Some(10),
					entries: hot,
					listing: None,
				},
			});
		}

		let pinned: Vec<Manga> = html
			.select("a.pinned-comic-card")
			.map(|items| {
				items
					.filter_map(|item| {
						card_manga(
							params,
							&item,
							"",
							".pinned-comic-title",
							".comic-thumbnail img",
						)
					})
					.take(12)
					.collect()
			})
			.unwrap_or_default();
		if !pinned.is_empty() {
			components.push(HomeComponent {
				title: Some("Editor's Choice".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: pinned,
					auto_scroll_interval: None,
				},
			});
		}

		let latest: Vec<MangaWithChapter> = html
			.select(".latest-releases .comic-card")
			.map(|items| {
				items
					.filter_map(|item| {
						let manga = card_manga(
							params,
							&item,
							"a.comic-card__cover",
							".comic-card__title",
							".comic-card__cover img",
						)?;
						let chapter = item.select_first(
							"a.chapter-item[href*='/chapter/']:not(.locked-chapter):not(.is-locked)",
						)?;
						let href = chapter.attr("abs:href").or_else(|| chapter.attr("href"))?;
						let label = chapter
							.select_first("label")
							.and_then(|el| el.text())
							.or_else(|| chapter.text())?;
						let date_uploaded = chapter
							.select_first(".chapter-date, time")
							.or_else(|| item.select_first(".chapter-date, time"))
							.and_then(|el| el.attr("datetime").or_else(|| el.text()))
							.and_then(|text| chapters::chapter_date(text.trim()));
						Some(MangaWithChapter {
							manga,
							chapter: Chapter {
								key: href.strip_prefix_or_self(&params.base_url).into(),
								chapter_number: chapter_number(&label),
								date_uploaded,
								title: Some(label),
								url: Some(href),
								..Default::default()
							},
						})
					})
					.collect()
			})
			.unwrap_or_default();
		if !latest.is_empty() {
			components.push(HomeComponent {
				title: Some("Latest Releases".into()),
				subtitle: None,
				value: HomeComponentValue::MangaChapterList {
					page_size: None,
					entries: latest,
					listing: None,
				},
			});
		}

		let novels: Vec<aidoku::Link> = html
			.select(".novels-section .novel-card")
			.map(|items| {
				items
					.filter_map(|item| {
						let mut manga = card_manga(
							params,
							&item,
							"a.novel-card-link",
							".novel-title",
							".novel-cover img",
						)?;
						let chapters = item
							.select_first(".chapter-badge")
							.and_then(|el| el.text())
							.map(|text| text.trim().to_string())
							.filter(|text| !text.is_empty());
						let blurb = item
							.select_first(".novel-excerpt")
							.and_then(|el| el.text())
							.map(|text| text.trim().to_string())
							.filter(|text| !text.is_empty());
						manga.description = match (chapters, blurb) {
							(Some(chapters), Some(blurb)) => Some(format!("{chapters} · {blurb}")),
							(chapters, blurb) => chapters.or(blurb),
						};
						Some(manga)
					})
					.map(Into::into)
					.collect()
			})
			.unwrap_or_default();
		if !novels.is_empty() {
			components.push(HomeComponent {
				title: Some("Latest Novels".into()),
				subtitle: None,
				value: HomeComponentValue::Scroller {
					entries: novels,
					listing: None,
				},
			});
		}

		Ok(HomeLayout { components })
	}
}

register_source!(
	Madara<RinkoComics>,
	Home,
	DeepLinkHandler,
	ImageRequestProvider
);
