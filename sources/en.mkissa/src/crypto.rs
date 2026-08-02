use aes_gcm::{
	Aes256Gcm, Nonce,
	aead::{Aead, KeyInit},
};
use aidoku::{
	Result,
	alloc::{String, Vec, vec},
	imports::std::current_date,
	prelude::*,
};
use base64::{Engine, engine::general_purpose};
use sha2::{Digest, Sha256};

// Build-specific values that must be updated if the site rotates its reader bundle.
pub const BUILD_ID: &str = "13";
pub const TS_BUCKET_MS: i64 = 5 * 60 * 1000;
const PART_A_HEX: &str = "f5dc46e6f42968c5ed0eab602d6ae8f2107991006f02876947e64fcb75d53da6";

fn hex_to_bytes(hex: &str) -> Vec<u8> {
	let bytes = hex.as_bytes();
	let mut out = Vec::with_capacity(hex.len() / 2);
	let mut i = 0;
	while i + 1 < bytes.len() {
		let hi = (bytes[i] as char).to_digit(16).unwrap_or(0);
		let lo = (bytes[i + 1] as char).to_digit(16).unwrap_or(0);
		out.push(((hi << 4) | lo) as u8);
		i += 2;
	}
	out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
	let mut hasher = Sha256::new();
	hasher.update(data);
	hasher.finalize().into()
}

pub fn sha256_hex(data: &str) -> String {
	let digest = sha256(data.as_bytes());
	let mut hex = String::with_capacity(64);
	for byte in digest {
		hex.push_str(&format!("{byte:02x}"));
	}
	hex
}

/// Derives the 32-byte signing key by XOR-ing the embedded part A with the
/// server-provided part B.
pub fn derive_signing_key(part_b: &str) -> Result<[u8; 32]> {
	let part_a = hex_to_bytes(PART_A_HEX);
	let part_b_bytes = general_purpose::STANDARD
		.decode(part_b)
		.or_else(|_| bail!("Invalid part B"))?;
	if part_b_bytes.len() < 32 {
		bail!("Part B too short");
	}
	let mut key = [0u8; 32];
	for (i, byte) in key.iter_mut().enumerate() {
		*byte = part_a[i] ^ part_b_bytes[i];
	}
	Ok(key)
}

fn now_ms() -> i64 {
	current_date() * 1000
}

fn ts_bucket() -> i64 {
	(now_ms() / TS_BUCKET_MS) * TS_BUCKET_MS
}

/// Builds the encrypted `aaReq` value sent with signed persisted queries.
pub fn build_aa_req(key: &[u8; 32], epoch: i64, query_hash: &str) -> Result<String> {
	let ts = ts_bucket();
	let payload = format!(
		r#"{{"v":1,"ts":{ts},"epoch":{epoch},"buildId":"{BUILD_ID}","qh":"{query_hash}"}}"#
	);

	let iv_source = format!("{epoch}:{BUILD_ID}:{query_hash}:{ts}");
	let iv_digest = sha256(iv_source.as_bytes());
	let iv = &iv_digest[..12];

	let cipher = Aes256Gcm::new(key.into());
	let ciphertext = cipher
		.encrypt(Nonce::from_slice(iv), payload.as_bytes())
		.or_else(|_| bail!("Encryption failed"))?;

	let mut out = vec![1u8];
	out.extend_from_slice(iv);
	out.extend_from_slice(&ciphertext);
	Ok(general_purpose::STANDARD.encode(out))
}

/// Decrypts the `tobeparsed` payload returned when page data is signed.
pub fn decrypt_tobe_parsed(value: &str, key: &[u8; 32]) -> Result<String> {
	let bytes = general_purpose::STANDARD
		.decode(value)
		.or_else(|_| bail!("Invalid payload"))?;
	if bytes.len() < 13 {
		bail!("Payload too short");
	}
	let iv = &bytes[1..13];
	let ciphertext = &bytes[13..];

	let cipher = Aes256Gcm::new(key.into());
	let plain = cipher
		.decrypt(Nonce::from_slice(iv), ciphertext)
		.or_else(|_| bail!("Decryption failed"))?;
	String::from_utf8(plain).or_else(|_| bail!("Invalid UTF-8 payload"))
}
