use std::fs;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::models::{Account, Customer, Transaction};

use super::{Result, Store, StoreError};

const EXPORT_FORMAT: &str = "atm-encrypted-export-v1";

#[derive(Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub format: String,
    pub exported_at: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Serialize)]
struct ExportDoc {
    exported_at: String,
    customers: Vec<Customer>,
}

pub(super) fn load_customers(conn: &Connection) -> rusqlite::Result<Vec<Customer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, locked, failed_attempts FROM customers ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Customer {
            id: r.get(0)?,
            name: r.get(1)?,
            locked: r.get::<_, i64>(2)? != 0,
            failed_attempts: r.get::<_, i64>(3)? as u32,
            accounts: Vec::new(),
        })
    })?;
    rows.collect()
}

pub(super) fn load_accounts(conn: &Connection, cust_id: i64) -> rusqlite::Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT number, balance FROM accounts WHERE customer_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![cust_id], |r| {
        Ok(Account {
            number: r.get(0)?,
            balance: r.get(1)?,
            transactions: Vec::new(),
        })
    })?;
    rows.collect()
}

pub(super) fn load_transactions(conn: &Connection, acc_id: i64) -> rusqlite::Result<Vec<Transaction>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, amount, balance_after, timestamp, note
         FROM transactions WHERE account_id = ?1 ORDER BY id DESC LIMIT 50",
    )?;
    let rows = stmt.query_map(params![acc_id], |r| {
        Ok(Transaction {
            id: r.get(0)?,
            kind: r.get(1)?,
            amount: r.get(2)?,
            balance_after: r.get(3)?,
            timestamp: r.get(4)?,
            note: r.get(5)?,
        })
    })?;
    rows.collect()
}

fn account_id_by_number(conn: &Connection, number: i32) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT id FROM accounts WHERE number = ?1",
        params![number],
        |r| r.get(0),
    )
}

impl Store {
    // Envelope encryption write path: load the whole database into a JSON
    // document (customers → accounts → embedded ledger), encrypt it with the
    // random DEK using a FRESH nonce each time (GCM must never reuse a nonce
    // with the same key), and store only base64 ciphertext + nonce on disk.
    // The DEK itself is never written here; admin_ops.rs keeps it wrapped.
    // The `export_lock` prevents two sessions writing at the same time and
    // leaving a half-written file — readers always see a complete snapshot.
    pub fn write_export(&self) -> Result<()> {
        let dek = {
            let guard = self.dek.lock().unwrap();
            match *guard {
                Some(dek) => dek,
                None => {
                    if !self.warn_missing_export() {
                        eprintln!("[WARN] Skipping JSON export: admin key file is missing.");
                    }
                    return Ok(());
                }
            }
        };

        let _lock = self.export_lock.lock().unwrap();
        let doc_json = {
            let conn = self.conn_guard();
            let mut customers = load_customers(&conn)?;
            for customer in &mut customers {
                customer.accounts = load_accounts(&conn, customer.id)?;
                for account in &mut customer.accounts {
                    account.transactions =
                        load_transactions(&conn, account_id_by_number(&conn, account.number)?)?;
                }
            }
            let doc = ExportDoc {
                exported_at: Self::now(),
                customers,
            };
            serde_json::to_string_pretty(&doc)
                .map_err(|e| StoreError::Crypto(format!("serialization failed: {e}")))?
        };

        let (nonce, ciphertext) = crypto::aes_encrypt(&dek, doc_json.as_bytes())?;
        let envelope = EncryptedEnvelope {
            format: EXPORT_FORMAT.to_string(),
            exported_at: Self::now(),
            nonce_b64: B64.encode(nonce),
            ciphertext_b64: B64.encode(ciphertext),
        };
        fs::write(self.paths.export(), serde_json::to_string_pretty(&envelope)?)?;
        Ok(())
    }

    // The decrypt path is the envelope flow in reverse: verify credentials →
    // re-derive KEK → unwrap DEK → decrypt the JSON document with AES-GCM →
    // write a plaintext copy. GCM also detects tampering: if a single byte
    // of the export file changed, decryption fails instead of lying to us.
    pub fn decrypt_export(&self, pin: &str, secret: &str, out_path: &str) -> Result<()> {
        if !self.verify_admin(pin, secret)? {
            return Err(StoreError::InvalidCredentials);
        }
        let salt: String = {
            let conn = self.conn_guard();
            conn.query_row("SELECT value FROM config WHERE key = 'kek_salt'", [], |r| r.get(0))?
        };
        let wrapped: String = {
            let conn = self.conn_guard();
            conn.query_row("SELECT value FROM config WHERE key = 'wrapped_dek'", [], |r| r.get(0))?
        };
        let kek = crypto::derive_kek(pin, secret, &salt)?;
        let dek = crypto::unwrap_key(&kek, &wrapped)?;

        let file = fs::read_to_string(self.paths.export())?;
        let envelope: super::EncryptedEnvelope = serde_json::from_str(&file)
            .map_err(|e| StoreError::Crypto(format!("corrupted export file: {e}")))?;
        if envelope.format != EXPORT_FORMAT {
            return Err(StoreError::Crypto("unknown export format".to_string()));
        }
        let nonce = B64
            .decode(&envelope.nonce_b64)
            .map_err(|e| StoreError::Crypto(format!("bad nonce: {e}")))?;
        let ciphertext = B64
            .decode(&envelope.ciphertext_b64)
            .map_err(|e| StoreError::Crypto(format!("bad ciphertext: {e}")))?;
        let plaintext = crypto::aes_decrypt(&dek, &nonce, &ciphertext)?;

        fs::write(out_path, plaintext)?;
        Ok(())
    }}
