#![no_std]
use aidoku::{Source, prelude::*};
use madtheme::{Impl, MadTheme, Params};

const BASE_URL: &str = "https://kaliscan.io";

struct KaliScan;

impl Impl for KaliScan {
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

register_source!(MadTheme<KaliScan>, ImageRequestProvider, DeepLinkHandler);
