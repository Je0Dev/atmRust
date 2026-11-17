use std::sync::Arc;

use atm_rust::app;
use atm_rust::store::Store;

// Thin entry point. The process ownership model is deliberately simple:
// - ONE `Store` is opened here (creating the schema / admin / seed data on
//   first run) and wrapped in an `Arc` so it can be shared by reference.
// - The TCP listener lives on its own background thread; every connected
//   client gets yet another thread (thread-per-connection). Threads are a
//   good fit here because a terminal session spends most of its time
//   blocked waiting for input — no async runtime needed for a handful of
//   users.
// - The console session runs here on the main thread with the same `Store`
//   and the same menu code, so a local user and remote users are peers.
fn main() {
    let store = match Store::open() {
        Ok(store) => Arc::new(store),
        Err(e) => {
            eprintln!("[ERROR] Failed to open database: {e}");
            return;
        }
    };

    app::start_tcp_server(Arc::clone(&store), app::SERVER_ADDR);
    app::run_console(store);
}
