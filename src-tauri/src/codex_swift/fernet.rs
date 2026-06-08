use aes::Aes128;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::codex_swift::types::DecryptedConfig;
use crate::error::AppError;

type Aes128CbcDec = cbc::Decryptor<Aes128>;
type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_KEY: &str = "SFqPT8UNq78DXB9gpEEHXyDcZoo4pIEwb0uEeomPp3k=";

pub fn decrypt_config_b64(config_b64: &str, key_b64: Option<&str>) -> Result<DecryptedConfig, AppError> {
    let env_key = std::env::var("APP_SECRET_KEY").ok();
    let key_str = key_b64
        .or(env_key.as_deref())
        .unwrap_or(DEFAULT_KEY)
        .trim();

    let key_bytes = decode_base64_url(key_str)
        .map_err(|e| AppError::Config(format!("fernet key base64 decode failed: {e}")))?;
    if key_bytes.len() != 32 {
        return Err(AppError::Config(format!(
            "fernet key must be 32 bytes, got {}",
            key_bytes.len()
        )));
    }

    let token_bytes = decode_fernet_token(config_b64.trim())
        .map_err(|e| AppError::Config(format!("fernet token base64 decode failed: {e}")))?;

    if token_bytes.len() < 1 + 8 + 16 + 32 {
        return Err(AppError::Config("fernet token too short".to_string()));
    }

    let version = token_bytes[0];
    if version != 0x80 {
        return Err(AppError::Config(format!("fernet unsupported version: {version:#x}")));
    }

    let hmac_start = token_bytes.len() - 32;
    let payload = &token_bytes[..hmac_start];
    let expected_hmac = &token_bytes[hmac_start..];

    // 前 16 字节 = signing key（用于 HMAC），后 16 字节 = encryption key（用于 AES）
    let signing_key = &key_bytes[..16];
    let encryption_key = &key_bytes[16..];

    let mut mac = HmacSha256::new_from_slice(signing_key)
        .map_err(|e| AppError::Config(format!("hmac init failed: {e}")))?;
    mac.update(payload);
    mac.verify_slice(expected_hmac)
        .map_err(|_| AppError::Config("fernet HMAC verification failed".to_string()))?;

    let iv = &token_bytes[9..25];
    let ciphertext = &token_bytes[25..hmac_start];

    let mut buf = ciphertext.to_vec();
    let decryptor = Aes128CbcDec::new(encryption_key.into(), iv.into());
    let plaintext = decryptor
        .decrypt_padded_mut::<cbc::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| AppError::Config(format!("AES decryption failed: {e}")))?;

    serde_json::from_slice::<DecryptedConfig>(plaintext)
        .map_err(|e| AppError::Config(format!("config JSON parse failed: {e}")))
}

fn decode_base64_url(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE
        .decode(input)
        .or_else(|_| URL_SAFE_NO_PAD.decode(input.trim_end_matches('=')))
}

fn decode_base64_any(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    decode_base64_url(input)
        .or_else(|_| STANDARD.decode(input))
        .or_else(|_| STANDARD_NO_PAD.decode(input.trim_end_matches('=')))
}

fn decode_fernet_token(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let decoded = decode_base64_any(input)?;
    if decoded.first() == Some(&0x80) {
        return Ok(decoded);
    }

    if let Ok(inner) = std::str::from_utf8(&decoded) {
        let inner = inner.trim();
        if inner.starts_with("gAAAA") {
            return decode_base64_url(inner);
        }
    }

    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_padded_fernet_keys() {
        let key = decode_base64_url("G3TUv4RtDiS4ZwYDcRnfDV6XMKCeqO7CxFiUFPwTcxI=")
            .expect("decode padded key");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn decodes_unpadded_fernet_keys() {
        let key = decode_base64_url("G3TUv4RtDiS4ZwYDcRnfDV6XMKCeqO7CxFiUFPwTcxI")
            .expect("decode unpadded key");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn decodes_base64_wrapped_fernet_token() {
        let plain_token = "gAAAAABmAAAAAA";
        let wrapped = STANDARD.encode(plain_token);
        let decoded = decode_fernet_token(&wrapped).expect("decode wrapped token");
        assert_eq!(decoded.first(), Some(&0x80));
    }
}
