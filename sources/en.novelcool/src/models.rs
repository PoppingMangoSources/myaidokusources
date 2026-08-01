use aidoku::alloc::{String, Vec};
use serde::Deserialize;

pub const DOMAIN: &str = "https://www.novelcool.com";
pub const API_URL: &str = "https://api.novelcool.com";
pub const PAGE_SIZE: i32 = 20;

pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

#[derive(Deserialize)]
pub struct ApiResponse<T> {
	pub error_code: Option<String>,
	pub error_msg: Option<String>,
	pub list: Option<Vec<T>>,
	pub info: Option<T>,
}

#[derive(Clone, Deserialize, Default)]
pub struct Book {
	#[serde(default)]
	pub id: String,
	pub book_id: Option<String>,
	pub url: Option<String>,
	#[serde(default)]
	pub name: String,
	pub author: Option<String>,
	pub artist: Option<String>,
	pub intro: Option<String>,
	pub completed: Option<String>,
	pub category_list: Option<Vec<String>>,
	pub last_chapter_id: Option<String>,
	pub last_chapter_title: Option<String>,
	pub modify_time: Option<String>,
	pub is_novel: Option<String>,
	#[serde(default)]
	pub cover: String,
}

impl Book {
	pub fn key(&self) -> &str {
		match &self.book_id {
			Some(id) if !id.is_empty() => id,
			_ => &self.id,
		}
	}

	pub fn is_novel(&self) -> bool {
		self.is_novel.as_deref() == Some("1")
	}
}

#[derive(Deserialize, Default)]
pub struct ApiChapter {
	#[serde(default)]
	pub id: String,
	#[serde(default)]
	pub title: String,
	pub order_id: Option<String>,
	pub last_modify: Option<String>,
	pub tf_time: Option<String>,
	pub is_locked: Option<serde_json::Value>,
	pub content: Option<String>,
	pub pic_list: Option<Vec<ApiPage>>,
}

#[derive(Deserialize)]
pub struct ApiPage {
	#[serde(default)]
	pub pic_path: String,
	#[serde(default)]
	pub order_id: i64,
}

pub fn is_locked(value: Option<&serde_json::Value>) -> bool {
	match value {
		Some(serde_json::Value::Bool(b)) => *b,
		Some(serde_json::Value::Number(n)) => n.as_i64() == Some(1),
		Some(serde_json::Value::String(s)) => s == "1" || s == "true",
		_ => false,
	}
}
