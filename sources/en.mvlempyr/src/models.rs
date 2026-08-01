use aidoku::{
	ContentRating, Link, LinkValue, Manga, MangaStatus, Viewer,
	alloc::{String, Vec, string::ToString, vec},
	prelude::*,
};

pub const DOMAIN: &str = "https://www.mvlempyr.io";
pub const CHAPTER_API: &str = "https://chap.heliosarchive.online";
pub const ASSETS_URL: &str = "https://assets.mvlempyr.app";

pub const CATALOGUE_PAGE_SIZE: i32 = 1000;
pub const CHAPTER_PAGE_SIZE: i32 = 500;
pub const LATEST_PAGE_SIZE: i32 = 30;

const ADULT_GENRES: &[&str] = &["adult", "smut", "yaoi", "yuri", "harem"];
const MATURE_GENRES: &[&str] = &["mature", "ecchi", "josei", "seinen"];

pub const GENRES: &[(&str, &str)] = &[
	("action", "Action"),
	("adult", "Adult"),
	("adventure", "Adventure"),
	("comedy", "Comedy"),
	("drama", "Drama"),
	("ecchi", "Ecchi"),
	("fan-fiction", "Fan-Fiction"),
	("fantasy", "Fantasy"),
	("gender-bender", "Gender Bender"),
	("harem", "Harem"),
	("historical", "Historical"),
	("horror", "Horror"),
	("josei", "Josei"),
	("martial-arts", "Martial Arts"),
	("mature", "Mature"),
	("mecha", "Mecha"),
	("mystery", "Mystery"),
	("psychological", "Psychological"),
	("romance", "Romance"),
	("school-life", "School Life"),
	("sci-fi", "Sci-fi"),
	("seinen", "Seinen"),
	("shoujo", "Shoujo"),
	("shounen", "Shounen"),
	("slice-of-life", "Slice of Life"),
	("smut", "Smut"),
	("sports", "Sports"),
	("supernatural", "Supernatural"),
	("tragedy", "Tragedy"),
	("wuxia", "Wuxia"),
	("xianxia", "Xianxia"),
	("xuanhuan", "Xuanhuan"),
	("yaoi", "Yaoi"),
	("yuri", "Yuri"),
];

#[derive(Clone, Default)]
pub struct Novel {
	pub name: String,
	pub slug: String,
	pub code: i64,
	pub rating: f32,
	pub reviews: f32,
	pub chapters: f32,
	pub genres: Vec<String>,
	pub author: Option<String>,
	pub status: Option<String>,
}

pub fn number_of(value: Option<&serde_json::Value>) -> Option<f32> {
	match value? {
		serde_json::Value::Number(n) => n.as_f64().map(|v| v as f32),
		serde_json::Value::String(s) => s.trim().parse::<f32>().ok(),
		_ => None,
	}
}

fn string_of(value: Option<&serde_json::Value>) -> Option<String> {
	match value? {
		serde_json::Value::String(s) => {
			let trimmed = s.trim();
			(!trimmed.is_empty()).then(|| trimmed.to_string())
		}
		_ => None,
	}
}

/// Renders a chapter number the way the site's urls do (no trailing `.0`).
pub fn format_number(value: f32) -> String {
	if (value as i64) as f32 == value {
		(value as i64).to_string()
	} else {
		value.to_string()
	}
}

impl Novel {
	pub fn from_value(value: &serde_json::Value) -> Option<Self> {
		let object = value.as_object()?;
		let name = string_of(object.get("name"))?;
		let slug = string_of(object.get("slug"))?;
		let code = number_of(object.get("novel-code"))? as i64;
		let genres = object
			.get("genre")
			.and_then(|g| g.as_array())
			.map(|items| {
				items
					.iter()
					.filter_map(|item| string_of(Some(item)))
					.collect()
			})
			.unwrap_or_default();
		Some(Self {
			name,
			slug,
			code,
			rating: number_of(object.get("average-review")).unwrap_or_default(),
			reviews: number_of(object.get("total-reviews")).unwrap_or_default(),
			chapters: number_of(object.get("total-chapters")).unwrap_or_default(),
			genres,
			author: string_of(object.get("author-name")),
			status: string_of(object.get("status")),
		})
	}

	pub fn cover(&self) -> String {
		format!("{ASSETS_URL}/images/600/{}.webp", self.code)
	}

	pub fn content_rating(&self) -> ContentRating {
		let lowered: Vec<String> = self.genres.iter().map(|g| g.to_lowercase()).collect();
		if lowered.iter().any(|g| ADULT_GENRES.contains(&g.as_str())) {
			ContentRating::NSFW
		} else if lowered.iter().any(|g| MATURE_GENRES.contains(&g.as_str())) {
			ContentRating::Suggestive
		} else {
			ContentRating::Safe
		}
	}

	pub fn manga_status(&self) -> MangaStatus {
		let status = self.status.as_deref().unwrap_or("").to_lowercase();
		if status.contains("complet") {
			MangaStatus::Completed
		} else if status.contains("ongoing") {
			MangaStatus::Ongoing
		} else if status.contains("hiatus") {
			MangaStatus::Hiatus
		} else if status.contains("drop") {
			MangaStatus::Cancelled
		} else {
			MangaStatus::Unknown
		}
	}

	pub fn status_matches(&self, wanted: &str) -> bool {
		matches!(
			(wanted, self.manga_status()),
			("ongoing", MangaStatus::Ongoing)
				| ("completed", MangaStatus::Completed)
				| ("hiatus", MangaStatus::Hiatus)
		)
	}

	pub fn genres_match(&self, included: &[String], excluded: &[String], match_all: bool) -> bool {
		let slugs: Vec<String> = self
			.genres
			.iter()
			.map(|g| g.to_lowercase().replace(' ', "-"))
			.collect();
		if excluded.iter().any(|id| slugs.iter().any(|s| s == id)) {
			return false;
		}
		if included.is_empty() {
			return true;
		}
		if match_all {
			included.iter().all(|id| slugs.iter().any(|s| s == id))
		} else {
			included.iter().any(|id| slugs.iter().any(|s| s == id))
		}
	}
}

impl From<Novel> for Manga {
	fn from(novel: Novel) -> Self {
		let content_rating = novel.content_rating();
		let status = novel.manga_status();
		let cover = novel.cover();
		Manga {
			key: novel.slug,
			title: novel.name,
			cover: Some(cover),
			authors: novel.author.map(|a| vec![a]),
			status,
			content_rating,
			viewer: Viewer::Vertical,
			tags: (!novel.genres.is_empty()).then_some(novel.genres),
			..Default::default()
		}
	}
}

impl From<Novel> for Link {
	fn from(novel: Novel) -> Self {
		let manga = Manga::from(novel);
		Link {
			title: manga.title.clone(),
			subtitle: None,
			image_url: manga.cover.clone(),
			value: Some(LinkValue::Manga(manga)),
		}
	}
}

#[derive(Clone, Copy)]
pub enum Sort {
	Popular,
	TopRated,
	MostReviewed,
	NewArrivals,
	MostChapters,
	Title,
}

impl Sort {
	pub fn from_index(index: i32) -> Self {
		match index {
			1 => Sort::TopRated,
			2 => Sort::MostReviewed,
			3 => Sort::NewArrivals,
			4 => Sort::MostChapters,
			5 => Sort::Title,
			_ => Sort::Popular,
		}
	}

	pub fn apply(self, novels: &mut [Novel]) {
		fn desc(a: f32, b: f32) -> core::cmp::Ordering {
			b.partial_cmp(&a).unwrap_or(core::cmp::Ordering::Equal)
		}
		match self {
			// Popularity blends the score with how many people rated it.
			Sort::Popular => {
				novels.sort_by(|a, b| desc(a.rating * a.reviews, b.rating * b.reviews))
			}
			Sort::TopRated => novels.sort_by(|a, b| desc(a.rating, b.rating)),
			Sort::MostReviewed => novels.sort_by(|a, b| desc(a.reviews, b.reviews)),
			Sort::NewArrivals => novels.sort_by(|a, b| b.code.cmp(&a.code)),
			Sort::MostChapters => novels.sort_by(|a, b| desc(a.chapters, b.chapters)),
			Sort::Title => novels.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
		}
	}
}
