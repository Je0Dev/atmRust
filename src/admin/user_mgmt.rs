use crate::session::Session;
use crate::store::{Result as StoreResult, Store};
use crate::utils;

pub fn list_customers(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let customers = store.all_customers()?;
    if customers.is_empty() {
        sess.print("[INFO] No customers found.\n")?;
        return Ok(());
    }
    for customer in &customers {
        let state = if customer.locked { "LOCKED" } else { "active" };
        sess.print(&format!(
            "\nCustomer #{} {} [{} | failed attempts: {}]\n",
            customer.id, customer.name, state, customer.failed_attempts
        ))?;
        for account in &customer.accounts {
            sess.print(&format!(
                "  Account {} | Balance: {:.2} EUR\n",
                account.number, account.balance
            ))?;
        }
    }
    Ok(())
}

pub fn create_customer(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let name = sess.prompt("Customer name: ")?;
    if name.trim().is_empty() {
        sess.print("[ERROR] Name cannot be empty.\n")?;
        return Ok(());
    }
    let pin = sess.prompt("Initial PIN (4 digits): ")?;
    if !utils::valid_pin(&pin) {
        sess.print("[ERROR] Invalid PIN. It must be exactly 4 digits.\n")?;
        return Ok(());
    }
    let number = store.create_customer(name.trim(), &pin)?;
    sess.print(&format!(
        "\n[OK] Customer '{name}' created with account number {number}.\n"
    ))?;
    Ok(())
}

pub fn reset_pin(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let raw = sess.prompt("Customer id: ")?;
    let Ok(cust_id) = raw.trim().parse::<i64>() else {
        sess.print("[ERROR] Invalid customer id.\n")?;
        return Ok(());
    };
    let Some(name) = store.customer_name(cust_id)? else {
        sess.print("[ERROR] Customer not found.\n")?;
        return Ok(());
    };
    let pin = sess.prompt("New PIN (4 digits): ")?;
    if !utils::valid_pin(&pin) {
        sess.print("[ERROR] Invalid PIN. It must be exactly 4 digits.\n")?;
        return Ok(());
    }
    store.change_pin(cust_id, &pin)?;
    sess.print(&format!("\n[OK] PIN reset for customer #{cust_id} ({name}).\n"))?;
    Ok(())
}

pub fn lock_unlock(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let raw = sess.prompt("Customer id: ")?;
    let Ok(cust_id) = raw.trim().parse::<i64>() else {
        sess.print("[ERROR] Invalid customer id.\n")?;
        return Ok(());
    };
    let Some(name) = store.customer_name(cust_id)? else {
        sess.print("[ERROR] Customer not found.\n")?;
        return Ok(());
    };

    let customers = store.all_customers()?;
    let locked = customers
        .iter()
        .find(|c| c.id == cust_id)
        .map(|c| c.locked)
        .unwrap_or(false);

    sess.print(&format!(
        "\n[INFO] Customer #{cust_id} ({name}) is currently {}.\n",
        if locked { "LOCKED" } else { "active" }
    ))?;
    let action = sess.prompt("Type 'lock' or 'unlock': ")?.to_lowercase();
    let new_state = match action.as_str() {
        "lock" => true,
        "unlock" => false,
        _ => {
            sess.print("[ERROR] Invalid action.\n")?;
            return Ok(());
        }
    };
    store.set_locked(cust_id, new_state)?;
    sess.print(&format!(
        "\n[OK] Customer #{cust_id} is now {}.\n",
        if new_state { "LOCKED" } else { "unlocked" }
    ))?;
    Ok(())
}

pub fn delete_account(sess: &mut dyn Session, store: &Store) -> StoreResult<()> {
    let Some(number) = utils::prompt_account_number(sess, "Account number to delete: ") else {
        return Ok(());
    };
    let Some(info) = store.find_account(number)? else {
        sess.print("[ERROR] Account not found.\n")?;
        return Ok(());
    };
    let confirm = sess.prompt(&format!(
        "Really delete account {number} of customer #{}? (y/N): ",
        info.cust_id
    ))?;
    if !confirm.eq_ignore_ascii_case("y") {
        sess.print("[INFO] Deletion cancelled.\n")?;
        return Ok(());
    }
    store.delete_account(info.cust_id, number)?;
    sess.print(&format!(
        "\n[OK] Account {number} deleted. The balance was credited to the customer's main account.\n"
    ))?;
    Ok(())
}
