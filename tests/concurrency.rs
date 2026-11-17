// Use case: several users at the same time (multithreading safety).
// Covers: concurrent withdrawals/deposits on the SAME account without lost
// updates, and the overdraft race (exactly one of two racing withdrawals
// may succeed).

mod common;

use std::sync::Arc;
use std::thread;

#[test]
fn concurrent_withdrawals_never_lose_money() {
    let store = Arc::new(common::open_store("conc_withdraw"));
    let (_, acc_id) = common::ids(&store, 10001); // Kydas, 100.00

    // Two users withdraw 30 EUR at the same time.
    let s2 = Arc::clone(&store);
    let t = thread::spawn(move || s2.withdraw(acc_id, 30.0));
    let r1 = store.withdraw(acc_id, 30.0).unwrap();
    let r2 = t.join().unwrap().unwrap();

    assert_eq!(r1, 70.0);
    assert_eq!(r2, 40.0);
    assert_eq!(common::balance(&store, acc_id), 40.0); // 100 - 30 - 30
}

#[test]
fn overdraft_race_lets_exactly_one_withdrawal_win() {
    let store = Arc::new(common::open_store("conc_overdraft"));
    let (_, acc_id) = common::ids(&store, 10002); // Mary, 250.00

    // Two withdrawals of the full balance race each other. The account must
    // never go negative: exactly one wins, the other gets InsufficientFunds.
    let s2 = Arc::clone(&store);
    let t = thread::spawn(move || s2.withdraw(acc_id, 250.0));
    let r1 = store.withdraw(acc_id, 250.0);
    let r2 = t.join().unwrap();

    let wins = [r1.is_ok(), r2.is_ok()].iter().filter(|w| **w).count();
    assert_eq!(wins, 1);
    assert_eq!(common::balance(&store, acc_id), 0.0);
}

#[test]
fn concurrent_deposits_all_land() {
    let store = Arc::new(common::open_store("conc_deposit"));
    let (_, acc_id) = common::ids(&store, 10005); // Nick, 0.00

    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(thread::spawn(move || s.deposit(acc_id, 1.0)));
    }
    for h in handles {
        h.join().unwrap().unwrap();
    }
    assert_eq!(common::balance(&store, acc_id), 10.0);
}

#[test]
fn n_clients_can_transact_at_once() {
    for clients in [2usize, 3, 10] {
        let store = Arc::new(common::open_store(&format!("n_clients_{clients}")));
        let (_, acc_id) = common::ids(&store, 10004); // Helen, 500.00

        let handles: Vec<_> = (0..clients)
            .map(|_| {
                let s = Arc::clone(&store);
                thread::spawn(move || {
                    s.withdraw(acc_id, 10.0).unwrap();
                    s.deposit(acc_id, 4.0).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        // Every client spent 6 net: 500 - 6*n.
        assert_eq!(common::balance(&store, acc_id), 500.0 - 6.0 * clients as f64);
    }
}

#[test]
fn transfer_between_customers_is_atomic() {
    let store = common::open_store("transfer");
    let (_, from_acc) = common::ids(&store, 10001); // Kydas, 100.00
    let (_, to_acc) = common::ids(&store, 10002); // Mary, 250.00

    let (new_from, to_number) = store.transfer(from_acc, 10002, 40.0).unwrap();
    assert_eq!(new_from, 60.0);
    assert_eq!(to_number, 10002);
    assert_eq!(common::balance(&store, to_acc), 290.0);

    // Same-account transfers and over-transfers are rejected.
    assert!(store.transfer(from_acc, 10001, 10.0).is_err());
    assert!(store.transfer(from_acc, 10002, 10_000.0).is_err());
    assert!(store.transfer(from_acc, 99999, 10.0).is_err());
}
