// Use case: admin seeds a batch of random users through the wizard.
// Covers per-user account counts / exact balances, random 4-digit PINs,
// the final admin re-authentication gate, the one-time report, automatic
// export update, and real multi-threaded use of the generated accounts.

mod common;

use std::sync::Arc;
use std::thread;

use atm_rust::app;
use atm_rust::crypto;
use atm_rust::session::MockSession;
use atm_rust::store::SeedSpec;

fn staged(count: usize, accounts: usize, balance: f64) -> Vec<SeedSpec> {
    (0..count)
        .map(|i| SeedSpec {
            name: format!("SeedUser{i}"),
            accounts,
            balance_per_account: balance,
        })
        .collect()
}

#[test]
fn bulk_seeder_creates_accounts_with_exact_balances_and_random_pins() {
    let store = common::open_store("seed_batch");
    let reports = store
        .seed_users_batch(&[
            SeedSpec { name: "Alpha".into(), accounts: 2, balance_per_account: 125.5 },
            SeedSpec { name: "Beta".into(), accounts: 1, balance_per_account: 0.01 },
        ])
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(store.all_customers().unwrap().len(), 7);
    for report in &reports {
        assert_eq!(report.pin.len(), 4);
        assert!(report.pin.chars().all(|c| c.is_ascii_digit()));
        for (number, balance) in &report.accounts {
            let (_, acc_id) = common::ids(&store, *number);
            let info = store.find_account(*number).unwrap().unwrap();
            assert_eq!(info.name, report.name);
            assert!(crypto::verify_secret(&report.pin, &info.pin_hash));
            assert_eq!(common::balance(&store, acc_id), *balance);
        }
    }
    assert_eq!(reports[0].accounts.len(), 2);
    assert_eq!(reports[1].accounts.len(), 1);
    assert_ne!(reports[0].pin, reports[1].pin);

    // The encrypted export updates automatically and stays ciphertext-only.
    let creds = common::admin_creds(&store);
    let out = store.paths().decrypted_export();
    assert!(!std::fs::read_to_string(store.paths().export()).unwrap().contains("Alpha"));
    store.decrypt_export(&creds.pin, &creds.secret, &out).unwrap();
    let plaintext = std::fs::read_to_string(&out).unwrap();
    assert!(plaintext.contains("Alpha"));
    assert!(plaintext.contains("Beta"));
}

#[test]
fn wizard_final_gate_rejects_wrong_admin_credentials() {
    let store = common::open_store("seed_gate_bad");
    let creds = common::admin_creds(&store);
    let input = [
        "2", &creds.pin, &creds.secret, // admin login
        "10", // seed users
        "1", "y", // one user, continue
        "2", "75.50", "y", // 2 accounts x 75.50 EUR, stage user
        "0000", "wrong-secret", // final gate fails
        "9", "3", // logout, exit
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();

    assert!(sess.output().contains("No users were created"));
    assert_eq!(store.all_customers().unwrap().len(), 5);
}

#[test]
fn wizard_commits_and_prints_one_time_report_then_user_can_log_in() {
    let store = common::open_store("seed_gate_ok");
    let creds = common::admin_creds(&store);
    let input = [
        "2", &creds.pin, &creds.secret, // admin login
        "10", // seed users
        "1", "y", // one user, continue
        "2", "75", "y", // 2 accounts x 75 EUR
        &creds.pin, &creds.secret, // final gate succeeds
        "9", "3", // logout, exit
    ];
    let mut sess = MockSession::new(&input);
    app::run_atm(&mut sess, &store).unwrap();
    let out = sess.output().to_string();

    assert!(out.contains("ONE-TIME report"));
    assert!(out.contains("The encrypted JSON export was updated automatically."));
    let pos = out.find("| PIN: ").unwrap() + "| PIN: ".len();
    let pin = &out[pos..pos + 4];
    assert!(pin.chars().all(|c| c.is_ascii_digit()));

    let customers = store.all_customers().unwrap();
    let seeded = customers.iter().find(|c| c.id > 5).unwrap();
    assert_eq!(seeded.accounts.len(), 2);
    assert!(seeded.accounts.iter().all(|a| (a.balance - 75.0).abs() < 0.01));

    // The generated user is a normal customer: login + transact works.
    let number = seeded.accounts[0].number.to_string();
    let login = ["1", &number, pin, "j", "3"];
    let mut sess = MockSession::new(&login);
    app::run_atm(&mut sess, &store).unwrap();
    assert!(sess.output().contains("[OK] Welcome,"));
}

#[test]
fn seeded_users_can_operate_in_parallel() {
    let store = Arc::new(common::open_store("seed_parallel"));
    let reports = store.seed_users_batch(&staged(2, 1, 100.0)).unwrap();
    let numbers: Vec<i32> = reports.iter().map(|r| r.accounts[0].0).collect();

    let handles: Vec<_> = numbers
        .iter()
        .map(|number| {
            let store = Arc::clone(&store);
            let number = *number;
            thread::spawn(move || {
                let info = store.find_account(number).unwrap().unwrap();
                store.withdraw(info.acc_id, 30.0).unwrap();
                store.deposit(info.acc_id, 5.0).unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    for number in numbers {
        let (_, acc_id) = common::ids(&store, number);
        assert_eq!(common::balance(&store, acc_id), 75.0);
    }
}
