use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::RngCore;

#[derive(Clone)]
pub struct SecretCipher {
    key_bytes: [u8; 32],
}

impl SecretCipher {
    pub fn from_hex(hex_key: &str) -> Result<Self, String> {
        let decoded = hex::decode(hex_key.trim())
            .map_err(|e| format!("Invalid DTA_MASTER_KEY hex encoding: {e}"))?;
        if decoded.len() != 32 {
            return Err(format!(
                "DTA_MASTER_KEY must decode to 32 bytes, got {}",
                decoded.len()
            ));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&decoded);
        Ok(Self { key_bytes })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let cipher = Aes256Gcm::new_from_slice(&self.key_bytes)
            .map_err(|e| format!("Cipher init failed: {e}"))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| format!("AES-GCM encryption failed: {e}"))?;
        let mut combined = Vec::with_capacity(12 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(format!("v1:{}", B64.encode(combined)))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<String, String> {
        let payload = encoded
            .strip_prefix("v1:")
            .ok_or_else(|| "Unsupported ciphertext version prefix".to_string())?;
        let combined = B64
            .decode(payload)
            .map_err(|e| format!("Ciphertext base64 decode failed: {e}"))?;
        if combined.len() < 13 {
            return Err("Ciphertext payload too short".to_string());
        }
        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.key_bytes)
            .map_err(|e| format!("Cipher init failed: {e}"))?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| "AES-GCM decryption failed".to_string())?;
        String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8 plaintext: {e}"))
    }
}
