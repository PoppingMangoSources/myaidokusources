#![no_std]
use aidoku::{
	Source,
	alloc::{String, string::ToString},
	imports::defaults::defaults_get,
	prelude::*,
};
use mangathemesia::{Impl, MangaThemesia, Params};

const DEFAULT_BASE_URL: &str = "https://rokaricomics.com";
const BASE_URL_KEY: &str = "baseUrl";

fn base_url() -> String {
	defaults_get::<String>(BASE_URL_KEY)
		.map(|url| url.trim().trim_end_matches('/').to_string())
		.filter(|url| url.starts_with("http"))
		.unwrap_or_else(|| DEFAULT_BASE_URL.into())
}

struct Rokaricomics;

impl Impl for Rokaricomics {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: base_url().into(),
			..Default::default()
		}
	}
}

register_source!(
	MangaThemesia<Rokaricomics>,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
