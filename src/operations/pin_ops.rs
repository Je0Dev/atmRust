use crate::crypto;
use crate::session::Session;
use crate::store::{Result as StoreResult, Store};
use crate::utils;

pub fn change_pin(sess: &mut dyn Session, store: &Store, cust_id: i64) -> StoreResult<()> {
    let old_pin = sess.prompt("Enter your old PIN: ")?;
    let pin_hash = store.customer_pin_hash(cust_id)?;
    if !crypto::verify_secret(&old_pin, &pin_hash) {
        sess.print("\n[ERROR] The old PIN is incorrect.\n")?;
        return Ok(());
    }

    let new_pin = sess.prompt("Enter your new PIN (4 digits): ")?;
    if !utils::valid_pin(&new_pin) {
        sess.print("\n[ERROR] Invalid PIN. It must be exactly 4 digits.\n")?;
        return Ok(());
    }
    let confirm = sess.prompt("Confirm your new PIN: ")?;
    if confirm != new_pin {
        sess.print("\n[ERROR] PINs do not match.\n")?;
        return Ok(());
    }

    store.change_pin(cust_id, &new_pin)?;
    sess.print("\n[OK] PIN changed successfully.\n")?;
    Ok(())
}
