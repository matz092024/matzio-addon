//! Where torrent results and title metadata come from.

pub mod addon;
pub mod cinemeta;
pub mod knaben;
pub mod torbox;
pub mod zilean;

use serde_json::Value;

/// A 40-character hex info hash, lowercased.
fn valid_hash(hash: String) -> Option<String> {
    (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
}

/// APIs send numbers as numbers or strings.
fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as u64),
        _ => None,
    }
}
