use std::error::Error;
use std::fmt;

use rusqlite;
use serde_json;

#[derive(Debug)]
pub enum StoreError {
    InsufficientFunds,
    AccountNotFound,
    LastAccount,
    MissingKeyFile,
    InvalidCredentials,
    CustomerNotFound,
    InvalidInput(String),
    Db(rusqlite::Error),
    Io(std::io::Error),
    Crypto(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::InsufficientFunds => write!(f, "insufficient funds"),
            StoreError::AccountNotFound => write!(f, "account not found"),
            StoreError::LastAccount => write!(f, "cannot delete the last account"),
            StoreError::MissingKeyFile => write!(f, "admin key file is missing"),
            StoreError::InvalidCredentials => write!(f, "credentials are invalid"),
            StoreError::CustomerNotFound => write!(f, "customer not found"),
            StoreError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            StoreError::Db(e) => write!(f, "database error: {e}"),
            StoreError::Io(e) => write!(f, "io error: {e}"),
            StoreError::Crypto(msg) => write!(f, "crypto error: {msg}"),
        }
    }
}

impl Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Crypto(format!("json error: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;
