// Use case: full menu flows driven through the real UI (run_atm) with a
// scripted user. Asserts on the exact messages a user would see.

mod common;

use atm_rust::app;
use atm_rust::session::MockSession;

#[test]
fn deposit_withdraw_and_check_balance_flow() {
    let store = common::open_store("ui_money");
    let input = [
        "1", // main: customer login
        "10001", "1234", // Kydas
        "f", "50", // deposit 50
        "1", // another action
        "e", "20", // withdraw 20
        "1", // another action
        "d", // show balance -> 130
        "3", // back to main screen
        "3", // exit
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert!(out.contains("[OK] Deposited 50.00 EUR"));
    assert!(out.contains("Withdrew 20.00 EUR"));
    assert!(out.contains("130.00 EUR"));
    assert!(out.contains("[OK] Thank you! Goodbye."));
}

#[test]
fn insufficient_funds_is_reported() {
    let store = common::open_store("ui_insufficient");
    let input = ["1", "10001", "1234", "e", "9999", "3", "3"];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("insufficient funds"));
}

#[test]
fn invalid_amount_is_rejected() {
    let store = common::open_store("ui_invalid_amount");
    let input = ["1", "10001", "1234", "f", "abc", "3", "3"];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("Invalid amount"));
}

#[test]
fn invalid_menu_choice_is_reported() {
    let store = common::open_store("ui_invalid_choice");
    let input = ["9", "", "3"];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("[ERROR] Invalid choice."));
}

#[test]
fn unknown_account_is_reported() {
    let store = common::open_store("ui_unknown_account");
    let input = ["1", "55555", "", "3"];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("No related account found"));
}

#[test]
fn multiple_accounts_ui_flow_including_delete_confirmation() {
    let store = common::open_store("ui_multi_account");

    // Create an extra account for Helen and remember its number from the output.
    let mut sess = MockSession::new(&["1", "10004", "3456", "a", "3", "3"]);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.take_output();
    assert!(out.contains("New account created with number: "));
    let extra: i32 = out
        .split("New account created with number: ")
        .nth(1)
        .unwrap()
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();

    // Switch to it, deposit, switch back, delete it (with confirmation),
    // and verify the money arrived on the main account.
    let input = [
        "1", "10004", "3456",
        "c", &extra.to_string(), "1", // switch to extra account
        "f", "75", "1", // deposit 75
        "c", "10004", "1", // switch back to main
        "b", &extra.to_string(), "y", "1", // delete extra, confirmed with y
        "d", // main balance is now 575
        "3", "3",
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert!(out.contains("[OK] Switched to account"));
    assert!(out.contains("Deposited 75.00 EUR"));
    assert!(out.contains("Account 60472") == false); // must not reference unknown numbers
    assert!(out.contains("was credited to your main account"));
    assert!(out.contains("575.00 EUR"));

    // Trying to delete the last remaining account is refused.
    let input = ["1", "10004", "3456", "b", "10004", "3", "3"];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("cannot delete your last account"));
}

#[test]
fn self_service_pin_change_flow() {
    let store = common::open_store("ui_pin_change");

    // Admin creates a customer so we have a known fresh account.
    let number = store.create_customer("PinTester", "4321").unwrap();

    let input = [
        "1", &number.to_string(), "4321",
        "i", "4321", "9999", "1111", // mismatch -> rejected
        "1", // another action
        "i", "4321", "9999", "9999", // match -> changed
        "3", "3",
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert!(out.contains("PINs do not match"));
    assert!(out.contains("[OK] PIN changed successfully."));

    // The new PIN must now be the valid one.
    let info = store.find_account(number).unwrap().unwrap();
    assert!(atm_rust::crypto::verify_secret("9999", &info.pin_hash));
}

#[test]
fn admin_menu_full_flow_through_ui() {
    let store = common::open_store("ui_admin");
    let creds = common::admin_creds(&store);

    let input = [
        "2", // main: admin login
        &creds.pin, &creds.secret,
        "1", // list customers & accounts
        "2", "UiAdmin", "2468", // create customer
        "6", // recent transactions
        "7", // decrypt export (re-authentication inside)
        &creds.pin, &creds.secret,
        "9", // logout
        "3", // exit app
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert!(out.contains("[OK] Admin authenticated."));
    assert!(out.contains("Customer #1 Kydas"));
    assert!(out.contains("created with account number"));
    assert!(out.contains("Export decrypted successfully"));
    assert!(out.contains("[OK] Admin logged out."));

    // The decrypted file exists and is plaintext.
    let plaintext = std::fs::read_to_string(store.paths().decrypted_export()).unwrap();
    assert!(plaintext.contains("Kydas"));
}

#[test]
fn admin_bad_credentials_are_rejected() {
    let store = common::open_store("ui_admin_bad");
    let creds = common::admin_creds(&store);

    let input = [
        "2",
        "0000", &creds.secret, // attempt 1 fails
        &creds.pin, "wrong", // attempt 2 fails
        &creds.pin, &creds.secret, // attempt 3 succeeds
        "9", "3",
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output();
    assert_eq!(
        out.matches("[ERROR] Invalid admin credentials.").count(),
        2
    );
    assert!(out.contains("[OK] Admin authenticated."));
}
