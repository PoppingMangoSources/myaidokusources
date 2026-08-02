use crate::crypto::{BUILD_ID, build_aa_req, derive_signing_key, sha256_hex};
use crate::models::*;
use aidoku::{
	Result,
	alloc::{String, Vec, string::ToString},
	helpers::uri::encode_uri_component,
	imports::net::Request,
	prelude::*,
};
use serde::de::DeserializeOwned;

/// Sends a GraphQL POST request and returns the typed `data` payload.
pub fn make_request<T: DeserializeOwned>(query: &str, variables: serde_json::Value) -> Result<T> {
	let body = serde_json::to_vec(&serde_json::json!({ "query": query, "variables": variables }))
		.or_else(|_| bail!("Failed to build request body"))?;

	let response = Request::post(API_URL)?
		.header("Content-Type", "application/json")
		.header("Accept", "application/json")
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Origin", DOMAIN)
		.body(body)
		.send()?;

	let parsed: GraphQLResponse<T> = response.get_json_owned()?;

	if let Some(errors) = parsed.errors
		&& !errors.is_empty()
	{
		let message = errors
			.iter()
			.map(|e| e.message.as_str())
			.collect::<Vec<_>>()
			.join("\n");
		bail!("Mkissa returned an error: {message}");
	}

	parsed.data.ok_or_else(|| error!("Missing response data"))
}

#[derive(serde::Deserialize)]
struct ApiPagesData {
	#[serde(rename = "chapterPages")]
	chapter_pages: Option<ChapterPages>,
	tobeparsed: Option<String>,
}

#[derive(serde::Deserialize)]
struct ApiPagesResponse {
	data: Option<ApiPagesData>,
}

#[derive(serde::Deserialize)]
struct DecryptedPages {
	#[serde(rename = "chapterPages")]
	chapter_pages: Option<ChapterPages>,
	edges: Option<Vec<ChapterPageEdge>>,
}

/// Fetches chapter page urls through the signed persisted-query api.
///
/// This is an ordinary request, so a challenge surfaces to the app and is
/// presented in the page rather than disappearing into a background web view.
pub fn fetch_chapter_pages_via_api(
	manga_id: &str,
	chapter_string: &str,
) -> Result<Option<ChapterPages>> {
	let Some(bootstrap) = get_signing_bootstrap()? else {
		return Ok(None);
	};

	let key = derive_signing_key(&bootstrap.part_b)?;
	let query_hash = sha256_hex(PAGES_QUERY);
	let aa_req = build_aa_req(&key, bootstrap.epoch, &query_hash)?;

	let variables = serde_json::json!({
		"mangaId": manga_id,
		"translationType": "sub",
		"chapterString": chapter_string,
		"limit": 10,
		"offset": 0,
	});
	let extensions = serde_json::json!({
		"persistedQuery": { "version": 1, "sha256Hash": query_hash },
		"aaReq": aa_req,
	});

	let url = format!(
		"{API_URL}?query={}&variables={}&extensions={}",
		encode_uri_component(PAGES_QUERY),
		encode_uri_component(variables.to_string()),
		encode_uri_component(extensions.to_string()),
	);

	let response = Request::get(&url)?
		.header("Accept", "application/json, text/plain, */*")
		.header("Referer", &format!("{DOMAIN}/"))
		.header("Origin", DOMAIN)
		.send()?;

	if response.status_code() != 200 {
		return Ok(None);
	}

	let parsed: ApiPagesResponse = response.get_json_owned()?;
	let Some(data) = parsed.data else {
		return Ok(None);
	};

	if let Some(pages) = data.chapter_pages
		&& !pages.edges.is_empty()
	{
		return Ok(Some(pages));
	}

	if let Some(tobeparsed) = data.tobeparsed {
		let decrypted = crate::crypto::decrypt_tobe_parsed(&tobeparsed, &key)?;
		let parsed: DecryptedPages = serde_json::from_str(&decrypted)
			.or_else(|_| bail!("Failed to parse decrypted pages"))?;
		if let Some(pages) = parsed.chapter_pages {
			return Ok(Some(pages));
		}
		if let Some(edges) = parsed.edges {
			return Ok(Some(ChapterPages { edges }));
		}
	}

	Ok(None)
}

/// Retrieves the signing bootstrap (epoch + key material) from the site.
fn get_signing_bootstrap() -> Result<Option<SigningBootstrap>> {
	let url = format!("{DOMAIN}/client-crypto/v1/bootstrap?buildId={BUILD_ID}");
	let Ok(response) = Request::get(&url)?.send() else {
		return Ok(None);
	};
	if response.status_code() != 200 {
		return Ok(None);
	}
	let Ok(text) = response.get_string() else {
		return Ok(None);
	};
	let Some(json) = extract_aa_crypto(&text) else {
		return Ok(None);
	};
	Ok(serde_json::from_str(json).ok())
}

/// Extracts the `window.__aaCrypto = { ... }` JSON object from a bootstrap page.
fn extract_aa_crypto(body: &str) -> Option<&str> {
	let anchor = body.find("__aaCrypto")?;
	let rest = &body[anchor..];
	let start = rest.find('{')?;
	let end = rest[start..].find('}')? + start;
	Some(&rest[start..=end])
}
