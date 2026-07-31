use aidoku::{
	ContentRating, MangaStatus, Viewer,
	alloc::{String, Vec},
};
use serde::Deserialize;

pub const DOMAIN: &str = "https://scans.gg";
pub const API_URL: &str = "https://api.scans.gg";
pub const CDN_URL: &str = "https://cdn.scans.gg/uploads";

pub const SERIES_PAGE_SIZE: i32 = 21;
pub const LATEST_PAGE_SIZE: i32 = 14;
pub const CHAPTER_PAGE_SIZE: i32 = 100;
pub const POPULAR_FETCH_SIZE: i32 = 50;

const ADULT_TAG_IDS: &[i64] = &[33, 34, 35, 38, 40, 44];
const MATURE_TAG_IDS: &[i64] = &[21, 24, 27, 28, 29, 30, 31, 37, 42];

#[derive(Deserialize)]
pub struct ResponseDto<T> {
	pub data: Option<T>,
	pub meta: Option<MetaDto>,
}

#[derive(Deserialize, Default)]
pub struct MetaDto {
	#[serde(default)]
	pub has_more: bool,
}

#[derive(Deserialize, Default)]
pub struct SeriesDto {
	pub id: i64,
	#[serde(default)]
	pub title: String,
	pub summary: Option<String>,
	pub cover: Option<String>,
	pub author: Option<Vec<String>>,
	pub artist: Option<Vec<String>>,
	pub tags: Option<Vec<i64>>,
	pub status: Option<i64>,
	#[serde(rename = "type")]
	pub kind: Option<i64>,
	pub content_rating: Option<i64>,
	pub themes: Option<Vec<String>>,
	pub chapters: Option<Vec<LatestChapterDto>>,
}

#[derive(Deserialize)]
pub struct GroupDto {
	pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct LatestChapterDto {
	pub id: Option<i64>,
	pub number: Option<serde_json::Value>,
	pub created_at: Option<String>,
	pub updated_at: Option<String>,
	pub group_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct ChapterDto {
	pub id: i64,
	pub number: Option<serde_json::Value>,
	pub title: Option<String>,
	pub created_at: Option<String>,
	pub group_id: Option<i64>,
	pub group: Option<GroupDto>,
	pub collab_groups: Option<Vec<GroupDto>>,
}

#[derive(Deserialize)]
pub struct PageListDto {
	pub chapter: Option<ChapterPagesDto>,
}

#[derive(Deserialize)]
pub struct ChapterPagesDto {
	pub id: Option<i64>,
	pub pages: Option<Vec<PageDto>>,
}

#[derive(Deserialize)]
pub struct PageDto {
	pub position: i64,
	pub path: String,
}

pub fn type_name(kind: Option<i64>) -> Option<&'static str> {
	match kind {
		Some(1) => Some("Comic"),
		Some(2) => Some("Manga"),
		Some(3) => Some("Manhwa"),
		Some(4) => Some("Manhua"),
		Some(5) => Some("Webtoon"),
		_ => None,
	}
}

pub fn viewer_for_type(kind: Option<i64>) -> Viewer {
	match kind {
		Some(1) => Viewer::LeftToRight,
		Some(2) => Viewer::RightToLeft,
		Some(3) | Some(4) | Some(5) => Viewer::Webtoon,
		_ => Viewer::Unknown,
	}
}

pub fn map_status(status: Option<i64>) -> MangaStatus {
	match status {
		Some(1) => MangaStatus::Ongoing,
		Some(2) => MangaStatus::Completed,
		Some(3) => MangaStatus::Hiatus,
		Some(4) => MangaStatus::Cancelled,
		Some(5) => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

pub fn derive_content_rating(content_rating: Option<i64>, tags: &[i64]) -> ContentRating {
	let tier = content_rating.unwrap_or(0);
	if tier >= 4 || tags.iter().any(|id| ADULT_TAG_IDS.contains(id)) {
		ContentRating::NSFW
	} else if tier >= 2 || tags.iter().any(|id| MATURE_TAG_IDS.contains(id)) {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

pub fn tag_name(id: i64) -> Option<&'static str> {
	Some(match id {
		1 => "Fantasy",
		2 => "Romance",
		3 => "Shoujo",
		4 => "Comedy",
		5 => "Drama",
		6 => "Slice Of Life",
		7 => "School Life",
		8 => "Thriller",
		9 => "Josei",
		10 => "Action",
		11 => "Seinen",
		12 => "Historical",
		13 => "Shounen",
		14 => "Sports",
		15 => "Supernatural",
		16 => "Adventure",
		17 => "Sci-fi",
		18 => "Martial Arts",
		19 => "Mystery",
		20 => "Horror",
		21 => "Mature",
		22 => "Psychological",
		23 => "Suspense",
		24 => "Gender Bender",
		25 => "Tragedy",
		26 => "Harem",
		27 => "Boys Love",
		28 => "Shounen Ai",
		29 => "Yaoi",
		30 => "Shoujo Ai",
		31 => "Yuri",
		32 => "Gourmet",
		33 => "Adult",
		34 => "Erotica",
		35 => "Smut",
		36 => "Music",
		37 => "Ecchi",
		38 => "Shotacon",
		39 => "Mecha",
		40 => "Hentai",
		41 => "Girls Love",
		42 => "Doujinshi",
		43 => "Mahou Shoujo",
		44 => "Lolicon",
		45 => "Award Winning",
		46 => "Avant Garde",
		47 => "Survival",
		48 => "Male Protagonist",
		49 => "Regression",
		_ => return None,
	})
}
