use crate::models::*;
use aidoku::{
	Result, alloc::Vec, helpers::uri::QueryParameters, imports::net::Request, prelude::*,
};
use serde::de::DeserializeOwned;

/// Sends a GraphQL POST request and returns the typed `data` payload.
pub fn make_request<T: DeserializeOwned>(query: &str, variables: serde_json::Value) -> Result<T> {
	// The api only answers GET requests that carry the document and its
	// variables in the query string; a posted body comes back empty.
	let variables = serde_json::to_string(&variables)
		.or_else(|_| bail!("Failed to build request variables"))?;
	let mut qs = QueryParameters::new();
	qs.push("variables", Some(&variables));
	qs.push("query", Some(query));

	let response = Request::get(format!("{API_URL}?{qs}"))?
		.header("Accept", "application/json")
		.header("Referer", &format!("{API_REFERER}/"))
		.header("Origin", API_REFERER)
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
