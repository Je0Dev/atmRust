use crate::session::Session;
use crate::store::{Result as StoreResult, Store};
use crate::utils;

pub fn create_new_account(sess: &mut dyn Session, store: &Store, cust_id: i64) -> StoreResult<()> {
    let number = store.create_account(cust_id)?;
    sess.print(&format!("\n[OK] New account created with number: {number}\n"))?;
    Ok(())
}

pub fn delete_account(
    sess: &mut dyn Session,
    store: &Store,
    cust_id: i64,
    acc_id: &mut i64,
) -> StoreResult<()> {
    let accounts = store.accounts_with_ids(cust_id)?;
    if accounts.len() <= 1 {
        sess.print("\n[ERROR] You cannot delete your last account.\n")?;
        return Ok(());
    }

    sess.print("\n[INFO] Your accounts:\n")?;
    for (_, number, balance) in &accounts {
        sess.print(&format!("  Account {number} | Balance: {balance:.2} EUR\n"))?;
    }

    let Some(number) = utils::prompt_account_number(sess, "Account number to delete: ") else {
        return Ok(());
    };

    let confirm = sess.prompt(&format!("Really delete account {number}? (y/N): "))?;
    if !confirm.eq_ignore_ascii_case("y") {
        sess.print("[INFO] Deletion cancelled.\n")?;
        return Ok(());
    }

    store.delete_account(cust_id, number)?;
    if let Some((current_number, _)) = store.account_info(*acc_id)? {
        if current_number == number {
            let accounts = store.accounts_with_ids(cust_id)?;
            if let Some((new_id, _, _)) = accounts.first() {
                *acc_id = *new_id;
            }
        }
    }
    sess.print(&format!(
        "\n[OK] Account {number} deleted. The balance was credited to your main account.\n"
    ))?;
    Ok(())
}

pub fn switch_account(
    sess: &mut dyn Session,
    store: &Store,
    cust_id: i64,
    acc_id: &mut i64,
) -> StoreResult<()> {
    let accounts = store.accounts_with_ids(cust_id)?;
    sess.print("\n[INFO] Available accounts:\n")?;
    for (_, number, balance) in &accounts {
        sess.print(&format!("  Account {number} | Balance: {balance:.2} EUR\n"))?;
    }

    let Some(number) = utils::prompt_account_number(sess, "Account to switch to: ") else {
        return Ok(());
    };

    for (id, acc_number, _) in &accounts {
        if *acc_number == number {
            *acc_id = *id;
            sess.print(&format!("\n[OK] Switched to account {number}.\n"))?;
            return Ok(());
        }
    }
    sess.print(&format!("\n[ERROR] Account {number} does not belong to you.\n"))?;
    Ok(())
}
