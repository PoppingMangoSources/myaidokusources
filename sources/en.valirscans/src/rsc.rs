//! Helpers for reading values out of a Next.js flight stream.
//!
//! Page data arrives either as raw stream rows (when requested with the `rsc`
//! header) or escaped inside script chunks of a full HTML document, so every
//! lookup is tried against both forms.

use aidoku::alloc::{String, Vec, string::ToString};
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

/// Collects every balanced JSON value that follows `marker`.
///
/// With `keep_marker` the slice starts at the marker itself, which is how the
/// site embeds objects like `{"series":...}`.
pub fn extract_all_by_marker<T: DeserializeOwned>(
	payload: &str,
	marker: &str,
	keep_marker: bool,
) -> Vec<T> {
	let mut values: Vec<T> = Vec::new();
	for text in [payload.to_string(), decode_escaped(payload)] {
		let mut offset = 0usize;
		while let Some(found) = text[offset..].find(marker) {
			let index = offset + found;
			let start = if keep_marker {
				index
			} else {
				index + marker.len()
			};
			if matches!(text.as_bytes().get(start), Some(b'{') | Some(b'['))
				&& let Some(raw) = slice_json(&text, start)
				&& let Ok(value) = serde_json::from_str::<T>(raw)
			{
				values.push(value);
			}
			offset = index + marker.len();
		}
		if !values.is_empty() {
			break;
		}
	}
	values
}
