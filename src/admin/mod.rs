use crate::session::Session;
use crate::store::{Result as StoreResult, Store};

mod maintenance;
mod user_mgmt;

type OpResult = Result<(), Box<dyn std::error::Error>>;

const MAX_ATTEMPTS: u32 = 3;

pub fn run(sess: &mut dyn Session, store: &Store) -> OpResult {
    sess.print("\n=== ADMIN LOGIN ===\n")?;
    if !authenticate(sess, store)? {
        return Ok(());
    }

    sess.print("\n[OK] Admin authenticated.\n")?;
    loop {
        sess.print("\n=== ADMIN MENU ===\n")?;
        sess.print("1. List all customers & accounts\n")?;
        sess.print("2. Create customer\n")?;
        sess.print("3. Reset customer PIN\n")?;
        sess.print("4. Lock / unlock customer\n")?;
        sess.print("5. Delete account\n")?;
        sess.print("6. View recent transactions\n")?;
        sess.print("7. Decrypt & view JSON export\n")?;
        sess.print("8. Rotate admin PIN & secret key\n")?;
        sess.print("9. Logout\n")?;
        sess.print("10. Seed random users\n")?;

        let choice = sess.prompt("Choice: ")?;
        let result: StoreResult<()> = match choice.as_str() {
            "1" => user_mgmt::list_customers(sess, store),
            "2" => user_mgmt::create_customer(sess, store),
            "3" => user_mgmt::reset_pin(sess, store),
            "4" => user_mgmt::lock_unlock(sess, store),
            "5" => user_mgmt::delete_account(sess, store),
            "6" => maintenance::recent_transactions(sess, store),
            "7" => maintenance::decrypt_export(sess, store),
            "8" => maintenance::rotate_credentials(sess, store),
            "9" => {
                sess.print("[OK] Admin logged out.\n")?;
                return Ok(());
            }
            "10" => maintenance::seed_users_wizard(sess, store),
            _ => {
                sess.print("[ERROR] Invalid choice.\n")?;
                Ok(())
            }
        };
        if let Err(e) = result {
            sess.print(&format!("[ERROR] {e}\n"))?;
        }
    }
}

fn authenticate(sess: &mut dyn Session, store: &Store) -> StoreResult<bool> {
    for attempt in 1..=MAX_ATTEMPTS {
        let pin = sess.prompt("Admin PIN: ")?;
        let secret = sess.prompt("Admin secret key: ")?;
        if store.verify_admin(&pin, &secret)? {
            return Ok(true);
        }
        if attempt < MAX_ATTEMPTS {
            sess.print("[ERROR] Invalid admin credentials.\n")?;
        }
    }
    sess.print("[ERROR] All attempts exhausted. Returning to main screen.\n")?;
    Ok(false)
}
