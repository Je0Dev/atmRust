// Use case: admin managing the system.
// Covers: credential verification, credential rotation (old creds die, new
// ones work, key file updated), creating customers, resetting PINs,
// locking/unlocking, deleting accounts, and decrypting the JSON export.

mod common;

use atm_rust::crypto;
use atm_rust::store::StoreError;

#[test]
fn admin_credentials_verify() {
    let store = common::open_store("admin_verify");
    let creds = common::admin_creds(&store);
    assert!(store.verify_admin(&creds.pin, &creds.secret).unwrap());
    assert!(!store.verify_admin("0000", &creds.secret).unwrap());
    assert!(!store.verify_admin(&creds.pin, "totally-wrong").unwrap());
}

#[test]
fn rotation_gives_new_pin_and_secret_and_kills_old_ones() {
    let store = common::open_store("admin_rotate");
    let old = common::admin_creds(&store);

    store.rotate_admin("4242", "new-secret-value").unwrap();

    // Old credentials must be dead, new ones must work…
    assert!(!store.verify_admin(&old.pin, &old.secret).unwrap());
    assert!(store.verify_admin("4242", "new-secret-value").unwrap());

    // …and the key file on disk must hold the new credentials.
    let on_disk = common::admin_creds(&store);
    assert_eq!(on_disk.pin, "4242");
    assert_eq!(on_disk.secret, "new-secret-value");
}

#[test]
fn admin_created_customer_can_log_in() {
    let store = common::open_store("admin_create");
    let number = store.create_customer("TestUser", "4321").unwrap();
    let info = store.find_account(number).unwrap().unwrap();
    assert!(crypto::verify_secret("4321", &info.pin_hash));
    assert!(!crypto::verify_secret("9999", &info.pin_hash));
}

#[test]
fn admin_reset_pin_and_lock_unlock() {
    let store = common::open_store("admin_reset_lock");
    let (cust_id, _) = common::ids(&store, 10002); // Mary

    // Reset the PIN, old one must stop working.
    store.change_pin(cust_id, "2468").unwrap();
    let hash = store.customer_pin_hash(cust_id).unwrap();
    assert!(!crypto::verify_secret("5678", &hash));
    assert!(crypto::verify_secret("2468", &hash));

    // Locking sets the flag, unlocking clears it and the attempt counter.
    store.set_locked(cust_id, true).unwrap();
    assert!(store.all_customers().unwrap().iter().any(|c| c.id == cust_id && c.locked));
    store.set_locked(cust_id, false).unwrap();
    let mary = store
        .all_customers()
        .unwrap()
        .into_iter()
        .find(|c| c.id == cust_id)
        .unwrap();
    assert!(!mary.locked);
    assert_eq!(mary.failed_attempts, 0);

    // Unknown customer ids must be rejected.
    assert!(matches!(
        store.set_locked(9999, true).unwrap_err(),
        StoreError::CustomerNotFound
    ));
}

#[test]
fn admin_delete_account_transfers_balance_to_main() {
    let store = common::open_store("admin_delete");
    let (cust_id, main_acc) = common::ids(&store, 10001); // Kydas, 100.00

    let extra = store.create_account(cust_id).unwrap();
    let (_, extra_acc) = common::ids(&store, extra);
    store.deposit(extra_acc, 60.0).unwrap();

    store.delete_account(cust_id, extra).unwrap();
    assert!(store.find_account(extra).unwrap().is_none());
    assert_eq!(common::balance(&store, main_acc), 160.0);

    // Deleting the now-last account must be refused.
    assert!(matches!(
        store.delete_account(cust_id, 10001).unwrap_err(),
        StoreError::LastAccount
    ));
}

#[test]
fn export_is_encrypted_and_only_admin_can_decrypt_it() {
    let store = common::open_store("admin_decrypt");
    let creds = common::admin_creds(&store);
    let export_file = std::fs::read_to_string(store.paths().export()).unwrap();

    // The snapshot must never contain plaintext customer data.
    assert!(!export_file.contains("Kydas"));
    assert!(export_file.contains("ciphertext_b64"));

    // Wrong credentials must be rejected.
    let out = store.paths().decrypted_export();
    assert!(matches!(
        store.decrypt_export("0000", "wrong", &out).unwrap_err(),
        StoreError::InvalidCredentials
    ));

    // Correct admin credentials produce a readable plaintext copy.
    store.decrypt_export(&creds.pin, &creds.secret, &out).unwrap();
    let plaintext = std::fs::read_to_string(&out).unwrap();
    assert!(plaintext.contains("Kydas"));
    assert!(plaintext.contains("10001"));
    assert!(!plaintext.contains("pin_hash")); // hashes are never exported
}

#[test]
fn recent_transactions_are_recorded_and_ordered() {
    let store = common::open_store("admin_transactions");
    let (_, acc_id) = common::ids(&store, 10001);

    store.deposit(acc_id, 10.0).unwrap();
    store.withdraw(acc_id, 5.0).unwrap();

    let rows = store.recent_transactions(10).unwrap();
    assert_eq!(rows.len(), 2);
    // Newest first: WITHDRAW must come before DEPOSIT.
    assert_eq!(rows[0].kind, "WITHDRAW");
    assert_eq!(rows[1].kind, "DEPOSIT");
    assert!(rows[0].timestamp >= rows[1].timestamp);
}
