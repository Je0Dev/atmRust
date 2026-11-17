use crate::session::Session;

pub fn parse_amount(input: &str) -> Option<f64> {
    let amount: f64 = input.trim().parse().ok()?;
    if amount > 0.0 && amount.is_finite() {
        Some((amount * 100.0).round() / 100.0)
    } else {
        None
    }
}

pub fn parse_account_number(input: &str) -> Option<i32> {
    let number: i32 = input.trim().parse().ok()?;
    (number > 0).then_some(number)
}

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit())
}

pub fn prompt_amount(sess: &mut dyn Session, label: &str) -> Option<f64> {
    let raw = match sess.prompt(label) {
        Ok(raw) => raw,
        Err(_) => return None,
    };
    match parse_amount(&raw) {
        Some(amount) => Some(amount),
        None => {
            let _ = sess.print("[ERROR] Invalid amount. Enter a positive number, e.g. 50 or 12.50.\n");
            None
        }
    }
}

pub fn prompt_account_number(sess: &mut dyn Session, label: &str) -> Option<i32> {
    let raw = match sess.prompt(label) {
        Ok(raw) => raw,
        Err(_) => return None,
    };
    match parse_account_number(&raw) {
        Some(number) => Some(number),
        None => {
            let _ = sess.print("[ERROR] Invalid account number.\n");
            None
        }
    }
}
