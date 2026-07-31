use aidoku::{
	MangaStatus, Viewer,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};
use serde::Deserialize;

pub const DOMAIN: &str = "https://chikari.moe";
pub const API_URL: &str = "https://chikari.moe/api";
pub const PAGE_SIZE: i32 = 24;

#[derive(Deserialize, Default)]
pub struct SeriesItem {
	pub slug: String,
	#[serde(default)]
	pub title: String,
	#[serde(rename = "type", default)]
	pub kind: String,
	#[serde(default)]
	pub status: String,
	#[serde(default)]
	pub is_nsfw: bool,
	#[serde(default)]
	pub cover_url: String,
	pub latest_chapter: Option<f32>,
	pub last_chapter_at: Option<String>,
}

#[derive(Deserialize)]
pub struct HomeRow {
	pub slug: String,
	#[serde(default)]
	pub items: Vec<SeriesItem>,
}

#[derive(Deserialize)]
pub struct HomeResponse {
	#[serde(default)]
	pub rows: Vec<HomeRow>,
}

#[derive(Deserialize)]
pub struct SeriesListResponse {
	#[serde(default)]
	pub items: Vec<SeriesItem>,
	#[serde(default)]
	pub total: i32,
}

#[derive(Deserialize)]
pub struct SeriesCredit {
	#[serde(default)]
	pub name: String,
	#[serde(default)]
	pub role: String,
}

#[derive(Deserialize)]
pub struct SeriesGenre {
	#[serde(default)]
	pub name: String,
}

#[derive(Deserialize)]
pub struct SeriesTag {
	#[serde(default)]
	pub name: String,
	#[serde(default)]
	pub is_spoiler: bool,
}

#[derive(Deserialize, Default)]
pub struct SeriesDetails {
	#[serde(default)]
	pub title: String,
	#[serde(rename = "type", default)]
	pub kind: String,
	#[serde(default)]
	pub status: String,
	#[serde(default)]
	pub is_nsfw: bool,
	#[serde(default)]
	pub cover_url: String,
	#[serde(default)]
	pub description: String,
	#[serde(default)]
	pub authors: Vec<SeriesCredit>,
	#[serde(default)]
	pub genres: Vec<SeriesGenre>,
	#[serde(default)]
	pub tags: Vec<SeriesTag>,
}

#[derive(Deserialize)]
pub struct ChapterItem {
	pub number: Option<f32>,
	#[serde(default)]
	pub volume: String,
	#[serde(default)]
	pub title: String,
	#[serde(default)]
	pub lang: String,
	#[serde(default)]
	pub created_at: String,
}

#[derive(Deserialize)]
pub struct ChapterListResponse {
	#[serde(default)]
	pub items: Vec<ChapterItem>,
}

#[derive(Deserialize)]
pub struct ChapterDetailsResponse {
	#[serde(default)]
	pub pages: Vec<String>,
}

/// Rewrites a `.webp` cover url to request a sized variant (e.g. `_400.webp`).
pub fn format_cover_url(url: &str, width: u32) -> String {
	let base = url.split('?').next().unwrap_or(url);
	match base.strip_suffix(".webp") {
		Some(prefix) => format!("{prefix}_{width}.webp"),
		None => base.to_string(),
	}
}

pub fn viewer_for_type(kind: &str) -> Viewer {
	match kind {
		"manhwa" | "manhua" => Viewer::Webtoon,
		"oel" => Viewer::LeftToRight,
		"manga" => Viewer::RightToLeft,
		_ => Viewer::Unknown,
	}
}

pub fn status_from(status: &str) -> MangaStatus {
	match status {
		"releasing" => MangaStatus::Ongoing,
		"completed" => MangaStatus::Completed,
		"hiatus" => MangaStatus::Hiatus,
		"cancelled" => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

/// Token used as a chapter key and API path segment.
pub fn chapter_token(number: Option<f32>) -> String {
	match number {
		Some(n) => {
			if (n as i64) as f32 == n {
				(n as i64).to_string()
			} else {
				n.to_string()
			}
		}
		None => "oneshot".to_string(),
	}
}
