use rusqlite::{params, Connection, OptionalExtension};

use crate::models::LoginInfo;

use super::{Result, Store, StoreError};

impl Store {
    pub fn find_account(&self, number: i32) -> Result<Option<LoginInfo>> {
        let conn = self.conn_guard();
        conn.query_row(
            "SELECT c.id, a.id, a.number, c.name, c.pin_hash, c.locked
             FROM accounts a JOIN customers c ON a.customer_id = c.id
             WHERE a.number = ?1",
            params![number],
            |r| {
                Ok(LoginInfo {
                    cust_id: r.get(0)?,
                    acc_id: r.get(1)?,
                    account_number: r.get(2)?,
                    name: r.get(3)?,
                    pin_hash: r.get(4)?,
                    locked: r.get::<_, i64>(5)? != 0,
                })
            },
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn account_info(&self, acc_id: i64) -> Result<Option<(i32, f64)>> {
        let conn = self.conn_guard();
        conn.query_row(
            "SELECT number, balance FROM accounts WHERE id = ?1",
            params![acc_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn accounts_with_ids(&self, cust_id: i64) -> Result<Vec<(i64, i32, f64)>> {
        let conn = self.conn_guard();
        let mut stmt = conn.prepare(
            "SELECT id, number, balance FROM accounts WHERE customer_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![cust_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StoreError::from)
    }

    // A new account is created inside a transaction that also records the
    // CREATE ledger row. Atomicity means the account and its first ledger
    // entry either both exist or neither does — a half-created account can
    // never show up, even if the process crashes mid-operation.
    pub fn create_account(&self, cust_id: i64) -> Result<i32> {
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;
        let number = generate_unique_number(&tx)?;
        tx.execute(
            "INSERT INTO accounts(customer_id, number, balance) VALUES (?1, ?2, 0)",
            params![cust_id, number],
        )?;
        let acc_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'CREATE', 0, 0, ?2, 'account created')",
            params![acc_id, Self::now()],
        )?;
        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(number)
    }

    // Deleting an account moves its balance to the customer's main account.
    // The credit, the ledger entry and the delete are one transaction: no
    // crash or concurrent reader can ever observe the money in limbo.
    pub fn delete_account(&self, cust_id: i64, number: i32) -> Result<()> {
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;

        let (acc_id, balance): (i64, f64) = tx
            .query_row(
                "SELECT id, balance FROM accounts WHERE number = ?1 AND customer_id = ?2",
                params![number, cust_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(StoreError::AccountNotFound)?;

        let total: i64 = tx.query_row(
            "SELECT COUNT(*) FROM accounts WHERE customer_id = ?1",
            params![cust_id],
            |r| r.get(0),
        )?;
        if total <= 1 {
            return Err(StoreError::LastAccount);
        }

        let main_acc_id: i64 = tx.query_row(
            "SELECT id FROM accounts WHERE customer_id = ?1 AND id != ?2 ORDER BY id LIMIT 1",
            params![cust_id, acc_id],
            |r| r.get(0),
        )?;

        tx.execute(
            "UPDATE accounts SET balance = balance + ?1 WHERE id = ?2",
            params![balance, main_acc_id],
        )?;
        let new_balance: f64 = tx.query_row(
            "SELECT balance FROM accounts WHERE id = ?1",
            params![main_acc_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'DELETE_TRANSFER', ?2, ?3, ?4, ?5)",
            params![
                main_acc_id,
                balance,
                new_balance,
                Self::now(),
                format!("Balance transfer from deleted account {number}")
            ],
        )?;

        tx.execute("DELETE FROM accounts WHERE id = ?1", params![acc_id])?;
        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(())
    }
}

// Account numbers are random 5-digit values. The uniqueness check happens
// against the very connection/transaction that will insert the number, so
// two concurrent sessions can never be handed the same number.
pub(super) fn generate_unique_number(conn: &Connection) -> Result<i32> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    loop {
        let number = rng.gen_range(10000..=99999);
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM accounts WHERE number = ?1",
                params![number],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(number);
        }
    }
}
