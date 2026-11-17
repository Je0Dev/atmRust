use rusqlite::{params, OptionalExtension};

use crate::models::{Transaction, TxnSummary};

use super::{Result, Store, StoreError};

impl Store {
    // Atomicity: the balance update AND its ledger row commit together.
    // The mutex already prevents two threads from updating at the exact same
    // nanosecond; the SQL transaction additionally guarantees that a crash
    // or an error halfway through leaves no partial state behind.
    pub fn deposit(&self, acc_id: i64, amount: f64) -> Result<f64> {
        if amount <= 0.0 {
            return Err(StoreError::InvalidInput("amount must be positive".to_string()));
        }
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE accounts SET balance = balance + ?1 WHERE id = ?2",
            params![amount, acc_id],
        )?;
        let balance: f64 =
            tx.query_row("SELECT balance FROM accounts WHERE id = ?1", params![acc_id], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'DEPOSIT', ?2, ?3, ?4, '')",
            params![acc_id, amount, balance, Self::now()],
        )?;
        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(balance)
    }

    // Important: the "is there enough money?" check runs INSIDE the
    // transaction, using the balance as it is at commit time. Checking
    // before acquiring the mutex would be a classic lost-update bug if two
    // sessions tried to spend the same balance concurrently.
    pub fn withdraw(&self, acc_id: i64, amount: f64) -> Result<f64> {
        if amount <= 0.0 {
            return Err(StoreError::InvalidInput("amount must be positive".to_string()));
        }
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;
        let balance: f64 =
            tx.query_row("SELECT balance FROM accounts WHERE id = ?1", params![acc_id], |r| r.get(0))?;
        if balance < amount {
            return Err(StoreError::InsufficientFunds);
        }
        tx.execute(
            "UPDATE accounts SET balance = balance - ?1 WHERE id = ?2",
            params![amount, acc_id],
        )?;
        let new_balance: f64 =
            tx.query_row("SELECT balance FROM accounts WHERE id = ?1", params![acc_id], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'WITHDRAW', ?2, ?3, ?4, '')",
            params![acc_id, amount, new_balance, Self::now()],
        )?;
        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(new_balance)
    }

    // A transfer touches two accounts and writes two ledger rows. Keeping
    // all four operations in one transaction is what makes a transfer a
    // single indivisible event: nobody can ever see only the debit side.
    pub fn transfer(&self, from_acc_id: i64, to_number: i32, amount: f64) -> Result<(f64, i32)> {
        if amount <= 0.0 {
            return Err(StoreError::InvalidInput("amount must be positive".to_string()));
        }
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;

        let from_number: i32 = tx.query_row(
            "SELECT number FROM accounts WHERE id = ?1",
            params![from_acc_id],
            |r| r.get(0),
        )?;
        if from_number == to_number {
            return Err(StoreError::InvalidInput(
                "cannot transfer to the same account".to_string(),
            ));
        }

        let (to_acc_id, _to_owner): (i64, String) = tx
            .query_row(
                "SELECT a.id, c.name FROM accounts a JOIN customers c ON a.customer_id = c.id
                 WHERE a.number = ?1",
                params![to_number],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(StoreError::AccountNotFound)?;

        let balance: f64 = tx.query_row(
            "SELECT balance FROM accounts WHERE id = ?1",
            params![from_acc_id],
            |r| r.get(0),
        )?;
        if balance < amount {
            return Err(StoreError::InsufficientFunds);
        }

        tx.execute(
            "UPDATE accounts SET balance = balance - ?1 WHERE id = ?2",
            params![amount, from_acc_id],
        )?;
        tx.execute(
            "UPDATE accounts SET balance = balance + ?1 WHERE id = ?2",
            params![amount, to_acc_id],
        )?;

        let from_balance: f64 = tx.query_row(
            "SELECT balance FROM accounts WHERE id = ?1",
            params![from_acc_id],
            |r| r.get(0),
        )?;
        let to_balance: f64 = tx.query_row(
            "SELECT balance FROM accounts WHERE id = ?1",
            params![to_acc_id],
            |r| r.get(0),
        )?;

        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'TRANSFER_OUT', ?2, ?3, ?4, ?5)",
            params![from_acc_id, amount, from_balance, Self::now(), format!("to account {to_number}")],
        )?;
        tx.execute(
            "INSERT INTO transactions(account_id, kind, amount, balance_after, timestamp, note)
             VALUES (?1, 'TRANSFER_IN', ?2, ?3, ?4, ?5)",
            params![to_acc_id, amount, to_balance, Self::now(), format!("from account {from_number}")],
        )?;

        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok((from_balance, to_number))
    }

    pub fn transactions_for(&self, acc_id: i64, limit: usize) -> Result<Vec<Transaction>> {
        let conn = self.conn_guard();
        let mut stmt = conn.prepare(
            "SELECT id, kind, amount, balance_after, timestamp, note
             FROM transactions WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![acc_id, limit as i64], |r| {
            Ok(Transaction {
                id: r.get(0)?,
                kind: r.get(1)?,
                amount: r.get(2)?,
                balance_after: r.get(3)?,
                timestamp: r.get(4)?,
                note: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StoreError::from)
    }

    pub fn recent_transactions(&self, limit: usize) -> Result<Vec<TxnSummary>> {
        let conn = self.conn_guard();
        let mut stmt = conn.prepare(
            "SELECT t.timestamp, t.kind, t.amount, t.balance_after, t.note, a.number, c.name
             FROM transactions t
             JOIN accounts a ON t.account_id = a.id
             JOIN customers c ON a.customer_id = c.id
             ORDER BY t.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(TxnSummary {
                timestamp: r.get(0)?,
                kind: r.get(1)?,
                amount: r.get(2)?,
                balance_after: r.get(3)?,
                note: r.get(4)?,
                account_number: r.get(5)?,
                owner: r.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StoreError::from)
    }
}
