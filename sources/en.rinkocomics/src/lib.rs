#![no_std]
use aidoku::{
	Chapter, ContentRating, FilterItem, FilterValue, HomeComponent, HomeComponentValue, HomeLayout,
	Manga, MangaWithChapter, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::string::StripPrefixOrSelf,
	imports::{html::Element, net::Request},
	prelude::*,
};
use madara::{Impl, Madara, Params};

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
	Some(Manga {
		key: href.strip_prefix_or_self(&params.base_url).into(),
		title: title.trim().to_string(),
		cover: image_from(element, image_selector),
		content_rating: ContentRating::Safe,
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

	fn get_home(&self, params: &Params) -> Result<HomeLayout> {
		let html = Request::get(&params.base_url)?.html()?;
		let mut components = Vec::new();

		let featured: Vec<Manga> = html
			.select(".hero-slider .slide")
			.map(|items| {
				items
					.filter_map(|item| card_manga(params, &item, "a", ".comic-title", "img"))
					.collect()
			})
			.unwrap_or_default();
		if !featured.is_empty() {
			components.push(HomeComponent {
				title: Some("Featured".into()),
				subtitle: None,
				value: HomeComponentValue::BigScroller {
					entries: featured,
					auto_scroll_interval: Some(6.0),
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

		let pinned: Vec<aidoku::Link> = html
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
					.map(Into::into)
					.collect()
			})
			.unwrap_or_default();
		if !pinned.is_empty() {
			components.push(HomeComponent {
				title: Some("Editor's Choice".into()),
				subtitle: None,
				value: HomeComponentValue::Scroller {
					entries: pinned,
					listing: None,
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
						Some(MangaWithChapter {
							manga,
							chapter: Chapter {
								key: href.strip_prefix_or_self(&params.base_url).into(),
								chapter_number: chapter_number(&label),
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
						card_manga(
							params,
							&item,
							"a.novel-card-link",
							".novel-title",
							".novel-cover img",
						)
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

		let genres = [
			("Action", "action"),
			("Comedy", "comedy"),
			("Drama", "drama"),
			("Fantasy", "fantasy"),
			("Josei", "josei"),
			("Romance", "romance"),
			("Shoujo", "shoujo"),
			("Smut", "smut"),
		]
		.into_iter()
		.map(|(title, id)| FilterItem {
			title: title.into(),
			values: Some(vec![FilterValue::MultiSelect {
				id: "genre[]".into(),
				included: vec![id.into()],
				excluded: Vec::new(),
			}]),
		})
		.collect();
		components.push(HomeComponent {
			title: Some("Genres".into()),
			subtitle: None,
			value: HomeComponentValue::Filters(genres),
		});

		Ok(HomeLayout { components })
	}
}

register_source!(
	Madara<RinkoComics>,
	Home,
	DeepLinkHandler,
	ImageRequestProvider
);
