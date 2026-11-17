use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use crate::session::{ConsoleSession, Session, TcpSession};
use crate::store::Store;

pub const SERVER_ADDR: &str = "127.0.0.1:8080";

// Comments here explain the multithreading model:
// - `main` starts a TCP listener on its own thread. Every incoming connection
//   gets yet another thread of its own (thread-per-client), so several users
//   can interact with the ATM at the same time without blocking each other.
// - All client threads share ONE `Arc<Store>`. The SQLite connection inside is
//   wrapped in a Mutex, which serializes every database access; combined with
//   SQL transactions for balance changes this prevents lost updates when two
//   users touch the same account simultaneously.
// - The console user runs on the main thread and shares the same store, so a
//   local user and a remote user can even work on the same account.
pub fn start_tcp_server(store: Arc<Store>, addr: &str) {
    match TcpListener::bind(addr) {
        Ok(listener) => {
            println!("[INFO] Server listening on {addr} — remote users can connect via telnet/ncat.");
            thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let store = Arc::clone(&store);
                            thread::spawn(move || {
                                if let Ok(mut sess) = TcpSession::new(stream) {
                                    let _ = run_atm(&mut sess, &store);
                                }
                            });
                        }
                        Err(_) => continue,
                    }
                }
            });
        }
        Err(e) => {
            println!("[WARN] TCP server unavailable ({e}). Console-only mode.");
        }
    }
}

pub fn run_console(store: Arc<Store>) {
    let mut console = ConsoleSession;
    let _ = run_atm(&mut console, &store);
}

pub fn run_atm(sess: &mut dyn Session, store: &Store) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = run_atm_inner(sess, store) {
        // Comments on transient IO errors:
        // When a TCP client disconnects (or piped stdin ends) the session
        // read/write calls fail with EOF/ConnectionReset/BrokenPipe. These are
        // normal ways for a session to end, not bugs — so they terminate the
        // session quietly instead of printing a scary [ERROR].
        let transient = e
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| is_transient_io(io))
            || e.downcast_ref::<crate::store::StoreError>()
                .is_some_and(|se| match se {
                    crate::store::StoreError::Io(io) => is_transient_io(io),
                    _ => false,
                });
        if transient {
            return Ok(());
        }
        let _ = sess.print(&format!("\n[ERROR] {e}\n"));
    }
    Ok(())
}

fn is_transient_io(io: &std::io::Error) -> bool {
    matches!(
        io.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    )
}

fn run_atm_inner(sess: &mut dyn Session, store: &Store) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        sess.clear()?;
        sess.print("=== ATM MAIN SCREEN ===\n")?;
        sess.print("1. Customer login\n")?;
        sess.print("2. Admin login\n")?;
        sess.print("3. Exit\n")?;

        let choice = sess.prompt("Choice: ")?;
        match choice.as_str() {
            "1" => match crate::auth::login(sess, store)? {
                Some((cust_id, acc_id)) => {
                    crate::operations::run_customer_session(sess, store, cust_id, acc_id)?
                }
                None => {
                    sess.prompt("\n[INFO] Press ENTER to continue...")?;
                }
            },
            "2" => {
                crate::admin::run(sess, store)?;
                sess.prompt("\n[INFO] Press ENTER to continue...")?;
            }
            "3" => {
                sess.print("\n[OK] Thank you! Goodbye.\n")?;
                return Ok(());
            }
            _ => {
                sess.print("[ERROR] Invalid choice.\n")?;
                sess.prompt("[INFO] Press ENTER to continue...")?;
            }
        }
    }
}
