use crate::session::Session;
use crate::store::{Result as StoreResult, Store};
use crate::utils;

pub fn check_account_number(account_number: i32, target: i32) -> bool {
    account_number == target
}

pub fn login(sess: &mut dyn Session, store: &Store) -> StoreResult<Option<(i64, i64)>> {
    const MAX_ATTEMPTS: u32 = 3;
    let mut attempts = 0;

    // Why the strike counter lives in the database, not in this variable:
    // `attempts` only bounds one login dialog, while `record_failed_attempt`
    // persists the real three-strikes count across sessions and TCP
    // connections. Without persistence, an attacker could disconnect after
    // two wrong PINs and start fresh — the DB-backed counter prevents that.
    // A successful login clears the counter (see `reset_attempts`).

    loop {
        let input = sess.prompt("Account number: ")?;
        let Some(account_number) = utils::parse_account_number(&input) else {
            sess.print("[ERROR] Invalid account number.\n")?;
            continue;
        };

        let Some(info) = store.find_account(account_number)? else {
            sess.print("[ERROR] No related account found.\n")?;
            continue;
        };

        if !check_account_number(info.account_number, account_number) {
            sess.print("[ERROR] No related account found.\n")?;
            continue;
        }

        if info.locked {
            sess.print("[ERROR] This account is locked. Please contact the admin.\n")?;
            return Ok(None);
        }

        let pin = sess.prompt("Enter PIN: ")?;
        if crate::crypto::verify_secret(&pin, &info.pin_hash) {
            store.reset_attempts(info.cust_id)?;
            sess.print(&format!("\n[OK] Welcome, {}!\n", info.name))?;
            return Ok(Some((info.cust_id, info.acc_id)));
        }

        attempts += 1;
        let now_locked = store.record_failed_attempt(info.cust_id)?;
        if now_locked {
            sess.print("[ERROR] Too many failed attempts. This account is now locked. Contact the admin.\n")?;
            return Ok(None);
        }
        sess.print("[ERROR] Wrong PIN.\n")?;
        if attempts >= MAX_ATTEMPTS {
            sess.print("[ERROR] All attempts exhausted. Returning to main screen.\n")?;
            return Ok(None);
        }
    }
}
