use crate::models::*;
use aidoku::{Result, alloc::Vec, imports::net::Request, prelude::*};
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
