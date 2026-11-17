use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::Rng;

use crate::store::StoreError;

// WHY Argon2id (not SHA-256, not plain PBKDF2): it is memory-hard, so an
// attacker with a GPU or a botnet pays a large RAM cost per guess instead of
// being able to try millions of PINs per second. `Argon2::default()` uses
// the OWASP-recommended parameters (m=64 MiB, t=3, p=4); we deliberately do
// not hand-tune them because weaker settings are an easy mistake.
// WHY a fresh random salt per hash: it stops two users with the same PIN (or
// one user after a PIN reset) from producing the same hash, and it makes
// precomputed rainbow tables useless.
// The result is a PHC string like `$argon2id$v=19$m=65536,t=3,p=4$salt$hash`
// — a self-describing format carrying the algorithm, parameters, salt and
// digest in one line, which is why we can simply store/parse it as a String.
pub fn hash_secret(secret: &str) -> Result<String, StoreError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| StoreError::Crypto(e.to_string()))
}

// Verification re-reads the parameters and salt from the PHC string, so old
// hashes keep working if the defaults ever change (unless the algorithm
// itself is deprecated). A malformed string simply fails closed.
pub fn verify_secret(secret: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(secret.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// PINs are 4 random digits (1000..=9999). Seeded demo users have fixed PINs;
// every generated user in the admin wizard gets one of these.
pub fn gen_pin() -> String {
    rand::thread_rng().gen_range(1000..=9999).to_string()
}

// The admin secret is a 24-character random string. The alphabet omits
// look-alike characters (0/O, 1/l/I) so a human can transcribe it correctly
// from the one-time printout into a telnet session.
pub fn gen_secret() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..24)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

// The DEK (data encryption key) is 256 bits of pure randomness. It never
// comes from a password, so guessing the admin credentials never attacks
// the export key directly — the credentials only unlock the *wrapped* DEK.
pub fn gen_dek() -> [u8; 32] {
    rand::random()
}

// KEK (key encryption key) derivation: Argon2id stretches the combination of
// admin PIN + secret key with the stored salt into a 32-byte key. The salt is
// stored in the DB, not hidden — its job is uniqueness, not secrecy. This
// "something you know (PIN/secret) + stored salt → key" step is why rotating
// credentials only needs to re-wrap the DEK, never re-encrypt the data.
pub fn derive_kek(pin: &str, secret: &str, salt_b64: &str) -> Result<[u8; 32], StoreError> {
    let salt = SaltString::from_b64(salt_b64)
        .map_err(|e| StoreError::Crypto(format!("invalid KEK salt: {e}")))?;
    let mut kek = [0u8; 32];
    let material = format!("{pin}:{secret}");
    Argon2::default()
        .hash_password_into(material.as_bytes(), salt.as_str().as_bytes(), &mut kek)
        .map_err(|e| StoreError::Crypto(format!("KEK derivation failed: {e}")))?;
    Ok(kek)
}

// Wrapping = encrypting the DEK with the KEK. We store the nonce next to the
// ciphertext (format `nonce:ciphertext`) because a random nonce is required
// to decrypt, but it is not a secret. Envelope encryption means the data is
// encrypted with the DEK and the DEK is encrypted ("wrapped") with the KEK:
// changing the KEK later only re-wraps 32 bytes instead of the whole export.
pub fn wrap_key(kek: &[u8; 32], dek: &[u8; 32]) -> Result<String, StoreError> {
    let (nonce, ciphertext) = aes_encrypt(kek, dek)?;
    Ok(format!("{}:{}", B64.encode(nonce), B64.encode(ciphertext)))
}

// Unwrapping reverses that: split, decode, decrypt, and insist on exactly 32
// bytes. GCM authentication means a wrong KEK produces an error, never a
// plausible-looking but wrong key.
pub fn unwrap_key(kek: &[u8; 32], wrapped: &str) -> Result<[u8; 32], StoreError> {
    let (nonce_b64, ct_b64) = wrapped
        .split_once(':')
        .ok_or_else(|| StoreError::Crypto("malformed wrapped key".to_string()))?;
    let nonce = B64.decode(nonce_b64)
        .map_err(|e| StoreError::Crypto(format!("bad wrapped nonce: {e}")))?;
    let ciphertext = B64.decode(ct_b64)
        .map_err(|e| StoreError::Crypto(format!("bad wrapped ciphertext: {e}")))?;
    let plaintext = aes_decrypt(kek, &nonce, &ciphertext)?;
    let dek: [u8; 32] = plaintext
        .try_into()
        .map_err(|_| StoreError::Crypto("unwrapped key has wrong length".to_string()))?;
    Ok(dek)
}

// WHY AES-256-GCM: it is an AEAD cipher — it provides confidentiality
// (nobody can read the export) AND integrity (any tampering is detected on
// decrypt). A fresh 12-byte random nonce is generated for every encryption:
// with GCM, reusing the same nonce under the same key would catastrophically
// leak information about both messages, so this line is security-critical.
pub fn aes_encrypt(key: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), StoreError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce_bytes: [u8; 12] = rand::random();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| StoreError::Crypto(format!("encryption failed: {e}")))?;
    Ok((nonce_bytes.to_vec(), ciphertext))
}

// GCM returns an authentication error if either the key, nonce or ciphertext
// was altered — that is the "integrity" half of AEAD doing its job.
pub fn aes_decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, StoreError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| StoreError::Crypto("decryption failed (wrong key or corrupted data)".to_string()))
}
