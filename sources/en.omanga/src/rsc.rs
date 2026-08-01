//! Helpers for reading values out of a Next.js flight stream.
//!
//! Page data arrives either as raw stream rows (when requested with the `rsc`
//! header) or escaped inside script chunks of a full HTML document, so every
//! lookup is tried against both forms.

use aidoku::{
	alloc::{String, string::ToString},
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

/// Slices the balanced JSON value that begins `offset` bytes after `marker`.
pub fn extract_at_marker<T: DeserializeOwned>(
	payload: &str,
	marker: &str,
	offset: usize,
) -> Option<T> {
	for text in [payload.to_string(), decode_escaped(payload)] {
		if let Some(index) = text.find(marker)
			&& let Some(raw) = slice_json(&text, index + offset)
			&& let Ok(value) = serde_json::from_str::<T>(raw)
		{
			return Some(value);
		}
	}
	None
}
