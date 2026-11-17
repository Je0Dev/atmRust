use crate::session::Session;
use crate::store::{Result as StoreResult, Store};

mod account_ops;
mod money_ops;
mod pin_ops;

type OpResult = Result<(), Box<dyn std::error::Error>>;

pub fn run_customer_session(
    sess: &mut dyn Session,
    store: &Store,
    cust_id: i64,
    mut acc_id: i64,
) -> OpResult {
    loop {
        let Some((account_number, _)) = store.account_info(acc_id)? else {
            sess.print("[ERROR] Current account is missing. Returning to main screen.\n")?;
            return Ok(());
        };
        let name = store
            .customer_name(cust_id)?
            .unwrap_or_else(|| "Customer".to_string());

        sess.clear()?;
        sess.print("=== ATM MENU ===\n")?;
        sess.print(&format!("[INFO] Welcome, {name}! | Current account: {account_number}\n"))?;
        sess.print("a. Create new account\n")?;
        sess.print("b. Delete account\n")?;
        sess.print("c. Switch account\n")?;
        sess.print("d. Show balance\n")?;
        sess.print("e. Withdraw\n")?;
        sess.print("f. Deposit\n")?;
        sess.print("g. Transfer\n")?;
        sess.print("h. Mini statement\n")?;
        sess.print("i. Change PIN\n")?;
        sess.print("j. Exit\n")?;

        let choice = sess.prompt("Choice: ")?.to_lowercase();
        let result: StoreResult<()> = match choice.as_str() {
            "a" => account_ops::create_new_account(sess, store, cust_id),
            "b" => account_ops::delete_account(sess, store, cust_id, &mut acc_id),
            "c" => account_ops::switch_account(sess, store, cust_id, &mut acc_id),
            "d" => money_ops::show_balance(sess, store, acc_id),
            "e" => money_ops::withdraw(sess, store, acc_id),
            "f" => money_ops::deposit(sess, store, acc_id),
            "g" => money_ops::transfer(sess, store, acc_id),
            "h" => money_ops::mini_statement(sess, store, acc_id),
            "i" => pin_ops::change_pin(sess, store, cust_id),
            "j" => {
                sess.print("\n[OK] Thank you! Goodbye.\n")?;
                return Ok(());
            }
            _ => {
                sess.print("[ERROR] Invalid choice.\n")?;
                Ok(())
            }
        };
        if let Err(e) = result {
            sess.print(&format!("[ERROR] {e}\n"))?;
        }

        sess.print("\n---\n")?;
        sess.print("1. Another action\n2. Exit\n3. Back to main screen\n")?;
        let post_action = sess.prompt("Choice: ")?;
        match post_action.as_str() {
            "2" => {
                sess.print("\n[OK] Thank you! Goodbye.\n")?;
                return Ok(());
            }
            "3" => return Ok(()),
            _ => continue,
        }
    }
}
