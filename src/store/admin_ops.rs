use std::fs;
use std::os::unix::fs::PermissionsExt;

use argon2::password_hash::{rand_core::OsRng, SaltString};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::crypto;

use super::{Result, Store, StoreError};

#[derive(Serialize, Deserialize)]
struct AdminKeyFile {
    pin: String,
    secret: String,
}

// On first run the admin's random PIN and secret key are printed once and
// written to a 0600 file. chmod 600 is deliberate: this file holds the
// plaintext credentials that unlock the wrapped export key (the DEK). In a
// real deployment this is the job of an HSM / secrets manager.
fn write_admin_key_file(path: &str, pin: &str, secret: &str) -> Result<()> {
    let key = AdminKeyFile {
        pin: pin.to_string(),
        secret: secret.to_string(),
    };
    let content = serde_json::to_string_pretty(&key)
        .map_err(|e| StoreError::Crypto(format!("key serialization failed: {e}")))?;
    fs::write(path, content)?;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

fn read_admin_key_file(path: &str) -> Option<AdminKeyFile> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

impl Store {
    // Key hierarchy bootstrap: a random DEK encrypts the export; a KEK
    // derived (via Argon2id) from the admin PIN + secret + a stored salt
    // wraps the DEK. Only the wrapped key is stored in the database, so a
    // database leak never yields the key that decrypts the export.
    pub(super) fn init_admin_and_dek(&self) -> Result<()> {
        let conn = self.conn_guard();
        let has_admin: i64 = conn.query_row("SELECT COUNT(*) FROM admin", [], |r| r.get(0))?;

        if has_admin == 0 {
            let pin = crypto::gen_pin();
            let secret = crypto::gen_secret();
            let pin_hash = crypto::hash_secret(&pin)?;
            let secret_hash = crypto::hash_secret(&secret)?;
            let salt = SaltString::generate(&mut OsRng).to_string();
            let dek = crypto::gen_dek();
            let kek = crypto::derive_kek(&pin, &secret, &salt)?;
            let wrapped = crypto::wrap_key(&kek, &dek)?;

            conn.execute(
                "INSERT INTO admin(id, pin_hash, secret_hash) VALUES (1, ?1, ?2)",
                params![pin_hash, secret_hash],
            )?;
            conn.execute(
                "INSERT INTO config(key, value) VALUES ('kek_salt', ?1)",
                params![salt],
            )?;
            conn.execute(
                "INSERT INTO config(key, value) VALUES ('wrapped_dek', ?1)",
                params![wrapped],
            )?;

            write_admin_key_file(&self.paths.admin_key(), &pin, &secret)?;
            *self.dek.lock().unwrap() = Some(dek);

            println!("[INFO] ==========================================================");
            println!("[INFO] First-run admin setup. These credentials are shown ONCE.");
            println!("[INFO] Admin PIN:        {pin}");
            println!("[INFO] Admin secret key: {secret}");
            let key_path = self.paths.admin_key();
            println!("[INFO] They are also stored in {key_path} (keep it safe).");
            println!("[INFO] ==========================================================");
        } else {
            let (pin_hash, secret_hash): (String, String) = conn.query_row(
                "SELECT pin_hash, secret_hash FROM admin WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let key = read_admin_key_file(&self.paths.admin_key()).ok_or(StoreError::MissingKeyFile)?;
            if !crypto::verify_secret(&key.pin, &pin_hash)
                || !crypto::verify_secret(&key.secret, &secret_hash)
            {
                return Err(StoreError::InvalidCredentials);
            }
            let salt: String =
                conn.query_row("SELECT value FROM config WHERE key = 'kek_salt'", [], |r| r.get(0))?;
            let wrapped: String = conn.query_row(
                "SELECT value FROM config WHERE key = 'wrapped_dek'",
                [],
                |r| r.get(0),
            )?;
            let kek = crypto::derive_kek(&key.pin, &key.secret, &salt)?;
            let dek = crypto::unwrap_key(&kek, &wrapped)?;
            *self.dek.lock().unwrap() = Some(dek);
        }
        Ok(())
    }

    pub fn verify_admin(&self, pin: &str, secret: &str) -> Result<bool> {
        let conn = self.conn_guard();
        let (pin_hash, secret_hash): (String, String) = conn.query_row(
            "SELECT pin_hash, secret_hash FROM admin WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(crypto::verify_secret(pin, &pin_hash) && crypto::verify_secret(secret, &secret_hash))
    }

    // Rotation derives a NEW KEK from the new credentials and re-wraps the
    // SAME DEK. The exported data itself is not decrypted or re-encrypted —
    // it stays valid because its DEK never changed; only the "key to the
    // key safe" is replaced.
    pub fn rotate_admin(&self, new_pin: &str, new_secret: &str) -> Result<()> {
        let pin_hash = crypto::hash_secret(new_pin)?;
        let secret_hash = crypto::hash_secret(new_secret)?;
        let salt = SaltString::generate(&mut OsRng).to_string();

        let old_dek = {
            let guard = self.dek.lock().unwrap();
            match *guard {
                Some(dek) => dek,
                None => crypto::gen_dek(),
            }
        };

        let kek = crypto::derive_kek(new_pin, new_secret, &salt)?;
        let wrapped = crypto::wrap_key(&kek, &old_dek)?;

        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE admin SET pin_hash = ?1, secret_hash = ?2 WHERE id = 1",
            params![pin_hash, secret_hash],
        )?;
        tx.execute(
            "INSERT INTO config(key, value) VALUES ('kek_salt', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![salt],
        )?;
        tx.execute(
            "INSERT INTO config(key, value) VALUES ('wrapped_dek', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![wrapped],
        )?;
        tx.commit()?;
        drop(conn);

        *self.dek.lock().unwrap() = Some(old_dek);
        write_admin_key_file(&self.paths.admin_key(), new_pin, new_secret)?;
        self.after_mutation();
        Ok(())
    }

}
