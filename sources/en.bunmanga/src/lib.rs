#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

const BASE_URL: &str = "https://bunmanga.com";

struct BunManga;

impl Impl for BunManga {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			..Default::default()
		}
	}
}

register_source!(
	Madara<BunManga>,
	Home,
	DeepLinkHandler,
	ImageRequestProvider
);
