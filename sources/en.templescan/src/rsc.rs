//! Helpers for reading values out of a Next.js flight stream.
//!
//! Page data arrives either as raw stream rows (when requested with the `rsc`
//! header) or escaped inside script chunks of a full HTML document, so every
//! lookup is tried against both forms.

use aidoku::{
	alloc::{String, Vec, string::ToString},
	prelude::*,
};
use serde::de::DeserializeOwned;

/// Slices one balanced JSON value starting at `start`.
///
/// The scan tracks string state so braces inside values cannot desync it.
pub fn slice_json(payload: &str, start: usize) -> Option<&str> {
	let mut depth = 0usize;
	let mut in_string = false;
	let mut escaped = false;
	for (index, byte) in payload.bytes().enumerate().skip(start) {
		if escaped {
			escaped = false;
			continue;
		}
		match byte {
			b'\\' => escaped = true,
			b'"' => in_string = !in_string,
			_ if in_string => {}
			b'{' | b'[' => depth += 1,
			b'}' | b']' => {
				depth -= 1;
				if depth == 0 {
					return payload.get(start..=index);
				}
			}
			_ => {}
		}
	}
	None
}

pub fn decode_escaped(payload: &str) -> String {
	payload.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// Finds the first value stored under `"key":` that satisfies `matches`.
fn scan_by_key<T, F>(payload: &str, key: &str, matches: &F) -> Option<T>
where
	T: DeserializeOwned,
	F: Fn(&T) -> bool,
{
	let marker = format!("\"{key}\":");
	let mut offset = 0usize;
	while let Some(found) = payload[offset..].find(&marker) {
		let start = offset + found + marker.len();
		if matches!(payload.as_bytes().get(start), Some(b'{') | Some(b'['))
			&& let Some(raw) = slice_json(payload, start)
			&& let Ok(value) = serde_json::from_str::<T>(raw)
			&& matches(&value)
		{
			return Some(value);
		}
		offset = start;
	}
	None
}

/// Looks a key up in the raw stream, then in its escaped-script form.
pub fn extract_by_key<T, F>(payload: &str, key: &str, matches: F) -> Option<T>
where
	T: DeserializeOwned,
	F: Fn(&T) -> bool,
{
	scan_by_key(payload, key, &matches)
		.or_else(|| scan_by_key(&decode_escaped(payload), key, &matches))
}

/// Returns the longest array of objects in the payload that `matches` accepts.
pub fn largest_array<T, F>(payload: &str, matches: F) -> Vec<T>
where
	T: DeserializeOwned,
	F: Fn(&[T]) -> bool,
{
	let mut best: Vec<T> = Vec::new();
	for text in [payload.to_string(), decode_escaped(payload)] {
		let mut offset = 0usize;
		while let Some(found) = text[offset..].find("[{") {
			let start = offset + found;
			if let Some(raw) = slice_json(&text, start)
				&& raw.len() > 40
				&& let Ok(value) = serde_json::from_str::<Vec<T>>(raw)
				&& value.len() > best.len()
				&& matches(&value)
			{
				best = value;
			}
			offset = start + 2;
		}
		if !best.is_empty() {
			break;
		}
	}
	best
}

/// Resolves a `"$<row>"` pointer back to that row's byte-counted text.
pub fn resolve_flight_ref(payload: &str, reference: &str) -> Option<String> {
	let row = reference.strip_prefix('$')?;

	fn row_text(text: &str, row: &str) -> Option<String> {
		let header = format!("{row}:T");
		let start = if text.starts_with(&header) {
			0
		} else {
			text.find(&format!("\n{header}"))? + 1
		};
		let after_row = start + header.len();
		let rest = text.get(after_row..)?;
		let comma = rest.find(',')?;
		let length = usize::from_str_radix(&rest[..comma], 16).ok()?;
		let body = rest.get(comma + 1..)?;
		// The header counts bytes, so walk chars until that many are consumed.
		let mut bytes = 0usize;
		let mut end = body.len();
		for (index, ch) in body.char_indices() {
			if bytes >= length {
				end = index;
				break;
			}
			bytes += ch.len_utf8();
		}
		body.get(..end).map(|s| s.to_string())
	}

	row_text(payload, row).or_else(|| row_text(&decode_escaped(payload), row))
}
