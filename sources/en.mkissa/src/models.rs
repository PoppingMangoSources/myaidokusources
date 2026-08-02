use aidoku::alloc::{String, Vec};
use serde::Deserialize;

pub const DOMAIN: &str = "https://mkissa.to";
pub const API_URL: &str = "https://api.mkissa.net/api";

pub const THUMBNAIL_CDN: &str = "https://wp.youtube-anime.com/aln.youtube-anime.com/";
pub const IMAGE_CDN: &str = "https://wp.youtube-anime.com";
pub const DEFAULT_IMAGE_SERVER: &str = "https://ytimgf.youtube-anime.com/";

pub const LIMIT: i32 = 20;

pub const POPULAR_QUERY: &str = "query($type: VaildPopularTypeEnumType!, $size: Int!, $page: Int, $dateRange: Int, $allowAdult: Boolean, $allowUnknown: Boolean) {\n  queryPopular(type: $type, size: $size, dateRange: $dateRange, page: $page, allowAdult: $allowAdult, allowUnknown: $allowUnknown) {\n    recommendations {\n      anyCard { _id name thumbnail englishName nativeName score availableChapters }\n      pageStatus { views }\n    }\n  }\n}";

pub const RANDOM_QUERY: &str = "query($format: String!, $allowAdult: Boolean) {\n  queryRandomRecommendation(format: $format, allowAdult: $allowAdult) {\n    _id name thumbnail englishName\n  }\n}";

pub const SEARCH_QUERY: &str = "query($search: SearchInput, $size: Int, $page: Int, $translationType: VaildTranslationTypeMangaEnumType, $countryOrigin: VaildCountryOriginEnumType) {\n  mangas(search: $search, limit: $size, page: $page, translationType: $translationType, countryOrigin: $countryOrigin) {\n    edges { _id name thumbnail englishName }\n  }\n}";

pub const LATEST_QUERY: &str = "query($search: SearchInput, $size: Int, $page: Int, $translationType: VaildTranslationTypeMangaEnumType, $countryOrigin: VaildCountryOriginEnumType) {\n  mangas(search: $search, limit: $size, page: $page, translationType: $translationType, countryOrigin: $countryOrigin) {\n    edges { _id name thumbnail englishName availableChapters availableChaptersDetail lastChapterDate }\n  }\n}";

pub const DETAILS_QUERY: &str = "query($id: String!) {\n  manga(_id: $id) { _id name thumbnail description authors genres tags status altNames englishName }\n}";

pub const CHAPTERS_QUERY: &str = "query($id: String!, $showId: String!) {\n  manga(_id: $id) { _id name availableChaptersDetail }\n  episodeInfos(showId: $showId, episodeNumStart: 0, episodeNumEnd: 9999) { episodeIdNum notes uploadDates }\n}";

#[derive(Deserialize)]
pub struct GraphQLResponse<T> {
	pub data: Option<T>,
	pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Deserialize)]
pub struct GraphQLError {
	pub message: String,
}

#[derive(Deserialize, Default)]
pub struct DateParts {
	pub year: Option<i32>,
	pub month: Option<i32>,
	pub date: Option<i32>,
	pub hour: Option<i32>,
	pub minute: Option<i32>,
	pub second: Option<i32>,
}

#[derive(Deserialize, Default)]
pub struct AvailableChaptersDetail {
	pub sub: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
pub struct LastChapterDate {
	pub sub: Option<DateParts>,
}

#[derive(Deserialize, Default)]
pub struct MangaCard {
	#[serde(rename = "_id")]
	pub id: String,
	#[serde(default)]
	pub name: String,
	pub thumbnail: Option<String>,
	#[serde(rename = "englishName")]
	pub english_name: Option<String>,
	#[serde(rename = "availableChaptersDetail")]
	pub available_chapters_detail: Option<AvailableChaptersDetail>,
	#[serde(rename = "lastChapterDate")]
	pub last_chapter_date: Option<LastChapterDate>,
}

impl MangaCard {
	pub fn display_title(&self) -> &str {
		match &self.english_name {
			Some(name) if !name.is_empty() => name,
			_ => &self.name,
		}
	}
}

#[derive(Deserialize)]
pub struct Recommendation {
	#[serde(rename = "anyCard")]
	pub any_card: Option<MangaCard>,
}

#[derive(Deserialize)]
pub struct QueryPopular {
	pub recommendations: Vec<Recommendation>,
}

#[derive(Deserialize)]
pub struct PopularData {
	#[serde(rename = "queryPopular")]
	pub query_popular: QueryPopular,
}

#[derive(Deserialize)]
pub struct RandomData {
	#[serde(rename = "queryRandomRecommendation")]
	pub query_random_recommendation: Option<Vec<MangaCard>>,
}

#[derive(Deserialize)]
pub struct MangaEdges {
	pub edges: Vec<MangaCard>,
}

#[derive(Deserialize)]
pub struct SearchData {
	pub mangas: MangaEdges,
}

#[derive(Deserialize, Default)]
pub struct MangaDetail {
	#[serde(default)]
	pub name: String,
	pub thumbnail: Option<String>,
	pub description: Option<String>,
	pub authors: Option<Vec<String>>,
	pub genres: Option<Vec<String>>,
	pub tags: Option<Vec<String>>,
	pub status: Option<String>,
	#[serde(rename = "englishName")]
	pub english_name: Option<String>,
}

#[derive(Deserialize)]
pub struct DetailsData {
	pub manga: MangaDetail,
}

#[derive(Deserialize, Default)]
pub struct ChaptersManga {
	#[serde(rename = "availableChaptersDetail")]
	pub available_chapters_detail: Option<AvailableChaptersDetail>,
}

#[derive(Deserialize, Default)]
pub struct UploadDates {
	pub sub: Option<String>,
}

#[derive(Deserialize)]
pub struct EpisodeInfo {
	#[serde(rename = "episodeIdNum")]
	pub episode_id_num: serde_json::Value,
	pub notes: Option<String>,
	#[serde(rename = "uploadDates")]
	pub upload_dates: Option<UploadDates>,
}

#[derive(Deserialize)]
pub struct ChaptersData {
	pub manga: ChaptersManga,
	#[serde(rename = "episodeInfos")]
	pub episode_infos: Option<Vec<EpisodeInfo>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum PictureUrl {
	Str(String),
	Obj { url: Option<String> },
}

impl PictureUrl {
	pub fn url(&self) -> Option<&str> {
		match self {
			PictureUrl::Str(s) => Some(s),
			PictureUrl::Obj { url } => url.as_deref(),
		}
	}
}

#[derive(Deserialize)]
pub struct ChapterPageEdge {
	#[serde(rename = "pictureUrlHead")]
	pub picture_url_head: Option<String>,
	#[serde(rename = "pictureUrls")]
	pub picture_urls: Option<Vec<PictureUrl>>,
}

#[derive(Deserialize)]
pub struct ChapterPages {
	pub edges: Vec<ChapterPageEdge>,
}

pub const GENRE_OPTIONS: &[&str] = &[
	"Action",
	"Adventure",
	"Comedy",
	"Drama",
	"Ecchi",
	"Fantasy",
	"Harem",
	"Historical",
	"Horror",
	"Isekai",
	"Josei",
	"Martial Arts",
	"Mecha",
	"Mystery",
	"Romance",
	"School",
	"Sci-Fi",
	"Seinen",
	"Shoujo",
	"Shounen",
	"Slice of Life",
	"Sports",
	"Supernatural",
	"Thriller",
	"Tragedy",
];
