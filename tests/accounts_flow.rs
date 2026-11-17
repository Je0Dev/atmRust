// Use case: multiple accounts per customer.
// Covers: creating extra accounts, switching between them, balances being
// independent, deleting an extra account (balance moves to the main account)
// and the guard that refuses to delete the last remaining account.

mod common;

use atm_rust::store::StoreError;
use common::ids;

#[test]
fn create_switch_and_delete_extra_account_keeps_money_safe() {
    let store = common::open_store("multi_account");
    let (cust_id, main_acc) = ids(&store, 10004); // Helen, seeded balance 500.00

    // Create a second account; it must start empty.
    let extra_number = store.create_account(cust_id).unwrap();
    assert_ne!(extra_number, 10004);
    let (_, extra_acc) = ids(&store, extra_number);
    assert_eq!(common::balance(&store, extra_acc), 0.0);

    // Money deposited into the extra account must NOT touch the main account.
    store.deposit(extra_acc, 75.0).unwrap();
    assert_eq!(common::balance(&store, extra_acc), 75.0);
    assert_eq!(common::balance(&store, main_acc), 500.0);

    // Deleting the extra account transfers its balance to the main account.
    store.delete_account(cust_id, extra_number).unwrap();
    assert!(store.find_account(extra_number).unwrap().is_none());
    assert_eq!(common::balance(&store, main_acc), 575.0);
}

#[test]
fn cannot_delete_the_last_account() {
    let store = common::open_store("last_account_guard");
    let (cust_id, _) = ids(&store, 10005); // Nick has exactly one account

    let err = store.delete_account(cust_id, 10005).unwrap_err();
    assert!(matches!(err, StoreError::LastAccount));
}

#[test]
fn cannot_delete_someone_elses_account() {
    let store = common::open_store("cross_customer_delete");
    // Helen must not be able to delete Kydas' account.
    let (helen_id, _) = ids(&store, 10004);
    let err = store.delete_account(helen_id, 10001).unwrap_err();
    assert!(matches!(err, StoreError::AccountNotFound));
    // Kydas' account is untouched.
    assert!(store.find_account(10001).unwrap().is_some());
}
