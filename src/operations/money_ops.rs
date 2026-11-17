use crate::models::Transaction;
use crate::session::Session;
use crate::store::{Result as StoreResult, Store, StoreError};
use crate::utils;

pub fn show_balance(sess: &mut dyn Session, store: &Store, acc_id: i64) -> StoreResult<()> {
    let Some((_, balance)) = store.account_info(acc_id)? else {
        return Err(StoreError::AccountNotFound);
    };
    sess.print(&format!("\n[OK] Your balance is: {balance:.2} EUR\n"))?;
    Ok(())
}

pub fn withdraw(sess: &mut dyn Session, store: &Store, acc_id: i64) -> StoreResult<()> {
    let Some(amount) = utils::prompt_amount(sess, "Amount to withdraw: ") else {
        return Ok(());
    };
    match store.withdraw(acc_id, amount) {
        Ok(new_balance) => {
            sess.print(&format!(
                "\n[OK] Withdrew {amount:.2} EUR. New balance: {new_balance:.2} EUR\n"
            ))?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn deposit(sess: &mut dyn Session, store: &Store, acc_id: i64) -> StoreResult<()> {
    let Some(amount) = utils::prompt_amount(sess, "Amount to deposit: ") else {
        return Ok(());
    };
    let new_balance = store.deposit(acc_id, amount)?;
    sess.print(&format!(
        "\n[OK] Deposited {amount:.2} EUR. New balance: {new_balance:.2} EUR\n"
    ))?;
    Ok(())
}

pub fn transfer(sess: &mut dyn Session, store: &Store, from_acc_id: i64) -> StoreResult<()> {
    let Some(amount) = utils::prompt_amount(sess, "Amount to transfer: ") else {
        return Ok(());
    };
    let Some(to_number) = utils::prompt_account_number(sess, "Target account number: ") else {
        return Ok(());
    };
    match store.transfer(from_acc_id, to_number, amount) {
        Ok((new_balance, _)) => {
            sess.print(&format!(
                "\n[OK] Transferred {amount:.2} EUR to account {to_number}. New balance: {new_balance:.2} EUR\n"
            ))?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn mini_statement(sess: &mut dyn Session, store: &Store, acc_id: i64) -> StoreResult<()> {
    let transactions: Vec<Transaction> = store.transactions_for(acc_id, 10)?;
    if transactions.is_empty() {
        sess.print("\n[INFO] No transactions recorded yet.\n")?;
        return Ok(());
    }
    sess.print("\n[INFO] Last transactions:\n")?;
    for txn in &transactions {
        sess.print(&format!(
            "  {} | {:<14} | {:>10.2} EUR | Balance: {:>10.2} EUR{}\n",
            txn.timestamp,
            txn.kind,
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
