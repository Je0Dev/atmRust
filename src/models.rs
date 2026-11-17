use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Transaction {
    pub id: i64,
    pub kind: String,
    pub amount: f64,
    pub balance_after: f64,
    pub timestamp: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub number: i32,
    pub balance: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub locked: bool,
    pub failed_attempts: u32,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone)]
pub struct LoginInfo {
    pub cust_id: i64,
    pub acc_id: i64,
    pub account_number: i32,
    pub name: String,
    pub pin_hash: String,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct TxnSummary {
    pub timestamp: String,
    pub kind: String,
    pub amount: f64,
    pub balance_after: f64,
    pub note: String,
    pub account_number: i32,
    pub owner: String,
}
