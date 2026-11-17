use rusqlite::{params, OptionalExtension};

use crate::crypto;
use crate::models::Customer;

use super::accounts::generate_unique_number;
use super::{Result, Store, StoreError};

const LOCKOUT_ATTEMPTS: u32 = 3;

impl Store {
    pub fn customer_name(&self, cust_id: i64) -> Result<Option<String>> {
        let conn = self.conn_guard();
        conn.query_row(
            "SELECT name FROM customers WHERE id = ?1",
            params![cust_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn customer_pin_hash(&self, cust_id: i64) -> Result<String> {
        let conn = self.conn_guard();
        conn.query_row(
            "SELECT pin_hash FROM customers WHERE id = ?1",
            params![cust_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::CustomerNotFound)
    }

    pub fn all_customers(&self) -> Result<Vec<Customer>> {
        let conn = self.conn_guard();
        let mut customers = super::export::load_customers(&conn)?;
        for customer in &mut customers {
            customer.accounts = super::export::load_accounts(&conn, customer.id)?;
        }
        Ok(customers)
    }

    pub fn create_customer(&self, name: &str, pin: &str) -> Result<i32> {
        let pin_hash = crypto::hash_secret(pin)?;
        let mut conn = self.conn_guard();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO customers(name, pin_hash) VALUES (?1, ?2)",
            params![name, pin_hash],
        )?;
        let cust_id = tx.last_insert_rowid();
        let number = generate_unique_number(&tx)?;
        tx.execute(
            "INSERT INTO accounts(customer_id, number, balance) VALUES (?1, ?2, 0)",
            params![cust_id, number],
        )?;
        tx.commit()?;
        drop(conn);
        self.after_mutation();
        Ok(number)
    }

    pub fn change_pin(&self, cust_id: i64, new_pin: &str) -> Result<()> {
        let pin_hash = crypto::hash_secret(new_pin)?;
        let conn = self.conn_guard();
        let changed = conn.execute(
            "UPDATE customers SET pin_hash = ?1 WHERE id = ?2",
            params![pin_hash, cust_id],
        )?;
        if changed == 0 {
            return Err(StoreError::CustomerNotFound);
        }
        drop(conn);
        self.after_mutation();
        Ok(())
    }

    // Lockout state is persisted in SQLite, not kept in a session variable.
    // Why: a wrong-PIN attacker could otherwise just disconnect and start a
    // new session to erase the strike count. Counting in the database makes
    // the three-strikes rule survive process restarts and every transport
    // (console or TCP) sees the same counter.
    pub fn record_failed_attempt(&self, cust_id: i64) -> Result<bool> {
        let conn = self.conn_guard();
        conn.execute(
            "UPDATE customers SET failed_attempts = failed_attempts + 1 WHERE id = ?1",
            params![cust_id],
        )?;
        let attempts: i64 = conn.query_row(
            "SELECT failed_attempts FROM customers WHERE id = ?1",
            params![cust_id],
            |r| r.get(0),
        )?;
        if attempts >= LOCKOUT_ATTEMPTS as i64 {
            conn.execute(
                "UPDATE customers SET locked = 1 WHERE id = ?1",
                params![cust_id],
            )?;
            drop(conn);
            self.after_mutation();
            return Ok(true);
        }
        Ok(false)
    }

    pub fn reset_attempts(&self, cust_id: i64) -> Result<()> {
        let conn = self.conn_guard();
        conn.execute(
            "UPDATE customers SET failed_attempts = 0 WHERE id = ?1",
            params![cust_id],
        )?;
        Ok(())
    }

    pub fn set_locked(&self, cust_id: i64, locked: bool) -> Result<()> {
        let conn = self.conn_guard();
        let changed = conn.execute(
            "UPDATE customers SET locked = ?1, failed_attempts = 0 WHERE id = ?2",
            params![locked as i64, cust_id],
        )?;
        if changed == 0 {
            return Err(StoreError::CustomerNotFound);
        }
        drop(conn);
        self.after_mutation();
        Ok(())
    }
}
