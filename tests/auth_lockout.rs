// Use case: wrong PIN 3 times -> account locked -> admin fixes it -> user retries.
// Covers: persisted failed-attempt counter, automatic lockout, locked accounts
// refusing login, admin unlock + PIN reset, old PIN failing and new PIN working.

mod common;

use atm_rust::app;
use atm_rust::session::MockSession;

#[test]
fn three_wrong_pins_lock_the_account() {
    let store = common::open_store("lockout");
    let (cust_id, _) = common::ids(&store, 10003); // George

    // The counter persists: two failures now…
    assert!(!store.record_failed_attempt(cust_id).unwrap());
    assert!(!store.record_failed_attempt(cust_id).unwrap());
    // …and a later session only needs one more failure to trigger the lock.
    assert!(store.record_failed_attempt(cust_id).unwrap());

    let info = store.find_account(10003).unwrap().unwrap();
    assert!(info.locked);
}

#[test]
fn locked_account_refuses_login() {
    let store = common::open_store("locked_login");
    let (cust_id, _) = common::ids(&store, 10005);

    for _ in 0..3 {
        store.record_failed_attempt(cust_id).unwrap();
    }

    let mut sess = MockSession::new(&["1", "10005", "7890", "", "3"]);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("This account is locked"));
    assert!(!sess.output().contains("Welcome"));
}

#[test]
fn admin_unlocks_and_resets_pin_then_user_retries() {
    let store = common::open_store("lockout_recovery");
    let (cust_id, _) = common::ids(&store, 10003);
    let creds = common::admin_creds(&store);

    // 1. User types the wrong PIN three times -> account locks.
    assert!(!store.record_failed_attempt(cust_id).unwrap());
    assert!(!store.record_failed_attempt(cust_id).unwrap());
    assert!(store.record_failed_attempt(cust_id).unwrap());

    // 2. Admin unlocks the customer and resets the PIN to 7777.
    store.set_locked(cust_id, false).unwrap();
    store.change_pin(cust_id, "7777").unwrap();

    // Unlocking must also clear the failed-attempt counter.
    let customers = store.all_customers().unwrap();
    let george = customers.iter().find(|c| c.id == cust_id).unwrap();
    assert!(!george.locked);
    assert_eq!(george.failed_attempts, 0);

    // 3. User retries: the old PIN no longer works…
    let hash = store.customer_pin_hash(cust_id).unwrap();
    assert!(!atm_rust::crypto::verify_secret("9012", &hash));
    // …but the new PIN does.
    assert!(atm_rust::crypto::verify_secret("7777", &hash));

    // 4. End to end through the UI: wrong PIN rejected, new PIN accepted.
    let mut sess = MockSession::new(&[
        "1", "10003", "9012", // old PIN -> rejected
        "10003", "7777", // new PIN -> accepted
        "d", // show balance
        "3", "3",
    ]);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert!(out.contains("[ERROR] Wrong PIN."));
    assert!(out.contains("[OK] Welcome, George!"));
    assert!(out.contains("50.00 EUR")); // George's seeded balance is intact
    let _ = creds; // key file is validated in admin_ops tests
}
