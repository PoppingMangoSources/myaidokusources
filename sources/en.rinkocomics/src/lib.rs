#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

const BASE_URL: &str = "https://rinkocomics.com";

struct RinkoComics;

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
}

register_source!(Madara<RinkoComics>, DeepLinkHandler, ImageRequestProvider);
