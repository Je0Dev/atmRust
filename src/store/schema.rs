use rand::Rng;
use rand::seq::SliceRandom;

use rusqlite::{params, Connection, OptionalExtension};

use super::accounts::generate_unique_number;
use super::{Result, Store};

#[derive(Debug, Clone)]
pub struct SeedSpec {
    pub name: String,
    pub accounts: usize,
    pub balance_per_account: f64,
}

#[derive(Debug, Clone)]
pub struct SeedReport {
    pub name: String,
    pub pin: String,
    pub accounts: Vec<(i32, f64)>,
}

// Table setup runs once at open time. `CREATE TABLE IF NOT EXISTS` makes
// every start tolerant of both a fresh directory and an existing database.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS customers(
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            pin_hash TEXT NOT NULL,
            locked INTEGER NOT NULL DEFAULT 0,
            failed_attempts INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS accounts(
            id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
            number INTEGER NOT NULL UNIQUE,
            balance REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS transactions(
            id INTEGER PRIMARY KEY,
            account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            amount REAL NOT NULL,
            balance_after REAL NOT NULL,
            timestamp TEXT NOT NULL,
            note TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS config(
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS admin(
            id INTEGER PRIMARY KEY CHECK (id = 1),
            pin_hash TEXT NOT NULL,
            secret_hash TEXT NOT NULL
        );",
    )?;
    Ok(())
}

// Seeds the five original demo customers exactly like the old hardcoded
// version did — but with the PINs hashed at seed time (Argon2id), never
// stored in plaintext.
pub fn seed_if_empty(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let seed = [
        ("1234", "Kydas", 100.0, 10001),
        ("5678", "Mary", 250.0, 10002),
        ("9012", "George", 50.0, 10003),
        ("3456", "Helen", 500.0, 10004),
        ("7890", "Nick", 0.0, 10005),
    ];
    for (pin, name, balance, number) in seed {
        let pin_hash = crate::crypto::hash_secret(pin)?;
        conn.execute(
            "INSERT INTO customers(name, pin_hash) VALUES (?1, ?2)",
            params![name, pin_hash],
        )?;
        let cust_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO accounts(customer_id, number, balance) VALUES (?1, ?2, ?3)",
            params![cust_id, number, balance],
        )?;
    }
    println!("[INFO] Seeded database with 5 demo customers.");
    Ok(())
}

// ---- Seed-random-users wizard support -------------------------------------

const SEED_NAME_POOL: &[&str] = &[
    "Alex", "Bella", "Chris", "Dana", "Elena", "Felix", "Gina", "Hans", "Iris", "Jonas", "Klara",
    "Liam", "Mara", "Noah", "Olga", "Paula", "Quinn", "Rita", "Sven", "Tara", "Ulf", "Vera",
    "Will", "Yara", "Zoe", "Anton", "Bruno", "Celia", "Doris", "Edgar",
];

// Picks a name for a generated user. A random pool name is preferred; if it
// would collide (with the DB or with users staged earlier in this batch) a
// numeric suffix is appended instead of failing the whole wizard.
impl Store {
    pub fn gen_seed_name(&self, staged: &mut Vec<String>) -> String {
        let conn = self.conn_guard();
        gen_seed_name(&conn, staged)
    }
}

fn gen_seed_name(conn: &Connection, staged: &mut Vec<String>) -> String {
    let mut rng = rand::thread_rng();
    let base = SEED_NAME_POOL.choose(&mut rng).unwrap_or(&"User").to_string();
    if name_free(conn, &base) && !staged.iter().any(|s| s == &base) {
        staged.push(base.clone());
        return base;
    }
    loop {
        let candidate = format!("{}{}", base, rng.gen_range(2..=999));
        if name_free(conn, &candidate) && !staged.iter().any(|s| s == &candidate) {
            staged.push(candidate.clone());
            return candidate;
        }
    }
}

fn name_free(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM customers WHERE name = ?1",
        params![name],
        |_| Ok(()),
    )
    .optional()
    .map(|opt| opt.is_none())
    .unwrap_or(false)
}

// The bulk seeder behind the admin wizard. Everything (every customer, every
// account, every random PIN hash) happens inside ONE transaction, so the
// commit is atomic: either all staged users exist after this call or none
// do. Random PINs are generated and hashed here so plaintext PINs only ever
// live in the returned one-time report.
impl Store {
    pub fn seed_users_batch(&self, specs: &[SeedSpec]) -> Result<Vec<SeedReport>> {
        for spec in specs {
            if spec.name.trim().is_empty() || spec.accounts == 0 {
                return Err(crate::store::StoreError::InvalidInput(
                    "each seeded user needs a name and at least one account".to_string(),
                ));
            }
            if spec.balance_per_account <= 0.0 || !spec.balance_per_account.is_finite() {
                return Err(crate::store::StoreError::InvalidInput(
                    "balance per account must be a positive finite amount".to_string(),
                ));
            }
        }

        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;

        let mut reports = Vec::with_capacity(specs.len());
        for spec in specs {
            let pin = crate::crypto::gen_pin();
            let pin_hash = crate::crypto::hash_secret(&pin)?;
            tx.execute(
                "INSERT INTO customers(name, pin_hash) VALUES (?1, ?2)",
                params![spec.name, pin_hash],
            )?;
            let cust_id = tx.last_insert_rowid();

            let mut accounts = Vec::with_capacity(spec.accounts);
            for _ in 0..spec.accounts {
                let number = generate_unique_number(&tx)?;
                tx.execute(
                    "INSERT INTO accounts(customer_id, number, balance) VALUES (?1, ?2, ?3)",
                    params![cust_id, number, spec.balance_per_account],
                )?;
                accounts.push((number, spec.balance_per_account));
            }
            reports.push(SeedReport {
                name: spec.name.clone(),
                pin,
                accounts,
            });
        }

        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(reports)
    }
}
