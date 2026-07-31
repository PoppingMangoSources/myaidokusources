use aidoku::{
	alloc::{String, Vec, vec},
	imports::defaults::defaults_get,
};

const CONTENT_TYPES_KEY: &str = "contentTypes";
const CONTENT_RATINGS_KEY: &str = "contentRatings";

pub fn content_types() -> Vec<String> {
	let types = defaults_get::<Vec<String>>(CONTENT_TYPES_KEY).unwrap_or_default();
	if types.is_empty() {
		vec!["manga".into(), "manhwa".into(), "manhua".into()]
	} else {
		types
	}
}

pub fn content_ratings() -> Vec<String> {
	let ratings = defaults_get::<Vec<String>>(CONTENT_RATINGS_KEY).unwrap_or_default();
	if ratings.is_empty() {
		vec!["safe".into(), "suggestive".into()]
	} else {
		ratings
	}
}

pub fn adult() -> bool {
	content_ratings()
		.iter()
		.any(|r| r == "erotica" || r == "pornographic")
}
