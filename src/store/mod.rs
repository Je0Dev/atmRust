use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

pub mod accounts;
pub mod admin_ops;
pub mod customers;
pub mod error;
pub mod export;
pub mod money;
pub mod schema;

pub use error::{Result, StoreError};
pub use export::EncryptedEnvelope;
pub use schema::{SeedReport, SeedSpec};

pub const DEFAULT_DATA_DIR: &str = "data";

// Comments on the path configuration:
// Every file the app touches (SQLite database, encrypted JSON export, admin
// key file) lives under one base directory. The default is "data"; tests pass
// their own temporary directory via Store::open_at so they never touch real
// user data and can run fully isolated in parallel.
#[derive(Debug, Clone)]
pub struct StorePaths {
    pub base: String,
}

impl StorePaths {
    pub fn new(base: &str) -> Self {
        StorePaths {
            base: base.to_string(),
        }
    }

    pub fn db(&self) -> String {
        format!("{}/atm.db", self.base)
    }

    pub fn export(&self) -> String {
        format!("{}/atm_export.json", self.base)
    }

    pub fn decrypted_export(&self) -> String {
        format!("{}/atm_export_decrypted.json", self.base)
    }

    pub fn admin_key(&self) -> String {
        format!("{}/admin.key", self.base)
    }
}

// Why one `Mutex` around the SQLite connection:
// Every session thread (console + one per TCP client) shares a single
// `Arc<Store>`. SQLite in this setup is one file opened by one connection,
// and the connection object is not safe for concurrent use. The mutex turns
// "many threads, one database" into "one writer at a time", which is
// completely safe and far more than fast enough for ATM-style traffic.
// Correctness *within* a single money operation is handled by SQL
// transactions in accounts.rs / money.rs; the mutex only makes sure those
// transactions never interleave.
pub struct Store {
    conn: Mutex<Connection>,
    // `dek` is the unwrapped data-encryption key for the JSON export. It is
    // only held in memory while the admin key file is available; a `None`
    // means "cannot encrypt exports right now" (see export.rs).
    dek: Mutex<Option<[u8; 32]>>,
    export_lock: Mutex<()>,
    export_warned: AtomicBool,
    paths: StorePaths,
}

impl Store {
    pub fn open() -> Result<Self> {
        Self::open_at("data")
    }

    pub fn open_at(base: &str) -> Result<Self> {
        let paths = StorePaths::new(base);
        fs::create_dir_all(&paths.base)?;
        let conn = Connection::open(paths.db())?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::init_schema(&conn)?;

        let store = Store {
            conn: Mutex::new(conn),
            dek: Mutex::new(None),
            export_lock: Mutex::new(()),
            export_warned: AtomicBool::new(false),
            paths,
        };

        store.init_admin_and_dek()?;
        schema::seed_if_empty(&store.conn_guard())?;
        store.write_export()?;
        Ok(store)
    }

    pub fn paths(&self) -> &StorePaths {
        &self.paths
    }

    pub(super) fn conn_guard(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    pub(super) fn now() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    // The "export trigger": every mutation funnels through here so the JSON
    // snapshot is always up to date. Export failures are warnings, not
    // errors — the database (source of truth) already committed successfully,
    // and a transient write failure must not make the user's action fail.
    pub(super) fn after_mutation(&self) {
        if let Err(e) = self.write_export() {
            eprintln!("[WARN] JSON export failed: {e}");
        }
    }

    pub(super) fn warn_missing_export(&self) -> bool {
        self.export_warned.swap(true, Ordering::Relaxed)
    }
}
