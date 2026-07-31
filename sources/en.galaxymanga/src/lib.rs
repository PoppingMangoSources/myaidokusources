#![no_std]
use aidoku::{Source, prelude::*};
use mangathemesia::{Impl, MangaThemesia, Params};

const BASE_URL: &str = "https://galaxymanga.io";

struct Galaxymanga;

impl Impl for Galaxymanga {
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
	MangaThemesia<Galaxymanga>,
	Home,
	ImageRequestProvider,
	DeepLinkHandler
);
