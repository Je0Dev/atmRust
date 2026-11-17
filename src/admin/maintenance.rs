use crate::crypto;
use crate::session::Session;
use crate::store::{Result as StoreResult, SeedSpec, Store, StoreError};
use crate::utils;

pub fn recent_transactions(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let rows = store.recent_transactions(20)?;
    if rows.is_empty() {
        sess.print("[INFO] No transactions recorded yet.\n")?;
        return Ok(());
    }
    sess.print("\n[INFO] Recent transactions:\n")?;
    for txn in &rows {
        sess.print(&format!(
            "  {} | {:<14} | Account {} ({}) | {:>10.2} EUR | Balance: {:>10.2} EUR{}\n",
            txn.timestamp,
            txn.kind,
            txn.account_number,
            txn.owner,
            txn.amount,
            txn.balance_after,
            if txn.note.is_empty() {
                String::new()
            } else {
                format!(" | {}", txn.note)
            }
        ))?;
    }
    Ok(())
}

pub fn decrypt_export(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    sess.print("\n[INFO] Admin re-authentication required to decrypt the export.\n")?;
    let pin = sess.prompt("Admin PIN: ")?;
    let secret = sess.prompt("Admin secret key: ")?;
    let out_path = store.paths().decrypted_export();
    match store.decrypt_export(&pin, &secret, &out_path) {
        Ok(()) => sess.print(&format!(
            "\n[OK] Export decrypted successfully. Written to {out_path}.\n"
        ))?,
        Err(StoreError::InvalidCredentials) => {
            sess.print("\n[ERROR] Invalid admin credentials. Export not decrypted.\n")?
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

pub fn rotate_credentials(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let confirm = sess.prompt("Generate a NEW admin PIN and secret key? (y/N): ")?;
    if !confirm.eq_ignore_ascii_case("y") {
        sess.print("[INFO] Rotation cancelled.\n")?;
        return Ok(());
    }

    let new_pin = crypto::gen_pin();
    let new_secret = crypto::gen_secret();
    store.rotate_admin(&new_pin, &new_secret)?;

    sess.print("\n[INFO] ==========================================================\n")?;
    sess.print("[INFO] Admin credentials rotated. Shown ONCE, store them now:\n")?;
    sess.print(&format!("[INFO] New Admin PIN:        {new_pin}\n"))?;
    sess.print(&format!("[INFO] New Admin secret key: {new_secret}\n"))?;
    sess.print("[INFO] They are also stored in data/admin.key (keep it safe).\n")?;
    sess.print("[INFO] ==========================================================\n")?;
    Ok(())
}

// Seed-random-users wizard: everything is staged in memory first. Nothing is
// written to the database until the admin has re-entered the admin PIN and
// secret key at the final gate — and the generated PINs are printed once,
// immediately after that single atomic batch commit.
pub fn seed_users_wizard(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    sess.print("\n=== SEED RANDOM USERS ===\n")?;
    let raw = sess.prompt("How many users to create? (0 to cancel): ")?;
    let Ok(count) = raw.trim().parse::<usize>() else {
        sess.print("[ERROR] Enter a whole number of users.\n")?;
        return Ok(());
    };
    if count == 0 {
        sess.print("[INFO] Seeding cancelled.\n")?;
        return Ok(());
    }
    if count > 100 {
        sess.print("[ERROR] A batch may contain at most 100 users.\n")?;
        return Ok(());
    }
    sess.print(&format!("\n[INFO] You are about to configure {count} user(s).\n"))?;
    let go = sess.prompt("Continue? (y/N): ")?;
    if !go.eq_ignore_ascii_case("y") {
        sess.print("[INFO] Seeding cancelled.\n")?;
        return Ok(());
    }

    let mut specs: Vec<SeedSpec> = Vec::with_capacity(count);
    let mut taken_names: Vec<String> = Vec::with_capacity(count);
    for index in 1..=count {
        let name = store.gen_seed_name(&mut taken_names);
        sess.print(&format!(
            "\n[INFO] User {index}/{count} — generated name: {name}\n"
        ))?;

        let raw_accounts = sess.prompt("Number of accounts: ")?;
        let Ok(accounts) = raw_accounts.trim().parse::<usize>() else {
            sess.print("[ERROR] Invalid number of accounts. Batch aborted.\n")?;
            return Ok(());
        };
        if accounts == 0 || accounts > 50 {
            sess.print("[ERROR] Enter 1–50 accounts. Batch aborted.\n")?;
            return Ok(());
        }

        let raw_money = sess.prompt("Exact money per account (EUR): ")?;
        let Some(balance_per_account) = utils::parse_amount(&raw_money) else {
            sess.print("[ERROR] Invalid amount. Batch aborted.\n")?;
            return Ok(());
        };

        sess.print(&format!(
            "[INFO] Staged: {name} — {accounts} account(s) x {balance_per_account:.2} EUR each.\n"
        ))?;
        let add = sess.prompt("Add this user? (y/N): ")?;
        if !add.eq_ignore_ascii_case("y") {
            sess.print("[INFO] User skipped.\n")?;
            continue;
        }
        specs.push(SeedSpec {
            name: name.clone(),
            accounts,
            balance_per_account,
        });
        sess.print(&format!(
            "[INFO] Running summary: {} user(s) staged for commit.\n",
            specs.len()
        ))?;
    }

    if specs.is_empty() {
        sess.print("[INFO] No users staged. Nothing to commit.\n")?;
        return Ok(());
    }

    // Final commit gate: re-enter admin credentials. A wrong key means the
    // staged batch is simply discarded — no partial inserts ever happen.
    sess.print(&format!(
        "\n[INFO] Final gate: commit {} staged user(s)?\n",
        specs.len()
    ))?;
    let pin = sess.prompt("Re-enter admin PIN: ")?;
    let secret = sess.prompt("Re-enter admin secret key: ")?;
    if !store.verify_admin(&pin, &secret)? {
        sess.print("[ERROR] Invalid admin credentials. No users were created.\n")?;
        return Ok(());
    }

    let reports = store.seed_users_batch(&specs)?;
    sess.print(&format!(
        "\n[OK] Committed {} user(s). Save this ONE-TIME report (PINs are not stored in plaintext):\n",
        reports.len()
    ))?;
    for report in &reports {
        sess.print(&format!(
            "\n  {name} | PIN: {pin}\n",
            name = report.name,
            pin = report.pin
        ))?;
        for (number, balance) in &report.accounts {
            sess.print(&format!(
                "    Account {number} | Balance: {balance:.2} EUR\n"
            ))?;
        }
    }
    sess.print("\n[INFO] The encrypted JSON export was updated automatically.\n")?;
    Ok(())
}
