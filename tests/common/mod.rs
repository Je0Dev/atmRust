// Shared helpers for the integration test suite.
//
// Every test runs against its own temporary directory (via Store::open_at)
// so tests are isolated and can run in parallel without touching each
// other's databases or the real "data" folder of the application.

use std::sync::atomic::{AtomicU32, Ordering};

use atm_rust::store::Store;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Creates a unique temporary base directory and returns its path as a String.
pub fn temp_base(label: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("opencode")
        .join("atm_tests")
        .join(format!("{label}_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().to_string()
}

/// Opens a fresh Store (with the 5 seeded demo customers) in a temp directory.
pub fn open_store(label: &str) -> Store {
    let base = temp_base(label);
    Store::open_at(&base).unwrap()
}

pub struct AdminCreds {
    pub pin: String,
    pub secret: String,
}

/// Reads the admin credentials from the admin key file of the given store.
pub fn admin_creds(store: &Store) -> AdminCreds {
    let text = std::fs::read_to_string(store.paths().admin_key()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    AdminCreds {
        pin: value["pin"].as_str().unwrap().to_string(),
        secret: value["secret"].as_str().unwrap().to_string(),
    }
}

/// Returns (customer_id, account_id) for a seeded account number.
pub fn ids(store: &Store, account_number: i32) -> (i64, i64) {
    let info = store.find_account(account_number).unwrap().unwrap();
    (info.cust_id, info.acc_id)
}

pub fn balance(store: &Store, acc_id: i64) -> f64 {
    store.account_info(acc_id).unwrap().unwrap().1
}
