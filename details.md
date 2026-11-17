# Details 
Each section explains the tricky parts in plain words first, then with the precise mechanics.
---

## 1. Encryption and decryption

![Simplified encryption between two ends](https://upload.wikimedia.org/wikipedia/commons/3/32/End-to-End_Encryption.png)

 [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:End-to-End_Encryption.png)

**What is the image about?:** Data goes
into a locked box as *ciphertext* (scrambled data), and only someone with the
right key can turn it back into *plaintext* (readable data). Our "box" is the
JSON export file; the admin is the only one with the key.


```
 readable JSON ──[ AES-256-GCM + DEK ]──> ciphertext in atm_export.json
 readable JSON <──[ AES-256-GCM + DEK ]── ciphertext   (admin only)
```

### The tools

- **Argon2id — turning a PIN into a hash.** A *hash* is a one-way transformation:
  easy to compute, practically impossible to reverse. Argon2id is a special
  kind called a *password-based key derivation function*, or KDF: it
  deliberately makes each guess **expensive in memory and time**. 
  Why? A PIN
  like `1234` has very few possibilities, so an attacker with a fast computer
  could try them all in seconds against a fast hash such as SHA-256. Argon2id
  forces each try to consume ~64 MB of RAM, which makes mass guessing
  impractical. Each credential gets its own random *salt* (a public random
  value mixed in), so two users with the same PIN get different hashes and
  precomputed attack tables ("rainbow tables") are useless. We store the
  result as a **PHC string**, e.g.
  `$argon2id$v=19$m=65536,t=3,p=4$salt$hash` — a self-describing one-line
  package containing the algorithm, its settings, salt and digest, so the
  verifier knows exactly how to check it.
- **AES-256-GCM — encrypting the export.** AES is the standard symmetric
  cipher (same key locks and unlocks). The `256` is the key size in bits. The
  `GCM` part makes it an **AEAD** cipher: *authenticated encryption with
  associated data*. In plain words it does two jobs at once: it keeps the
  contents secret **and** it seals them with a tamper-evident stamp. If one
  byte of the ciphertext is changed, decryption fails loudly instead of
  returning corrupted data that looks real. Every encryption uses a fresh
  random **nonce** (a "number used once"). With GCM, reusing a nonce under the
  same key is catastrophic — the code comment in `crypto.rs` calls this out
  because it is a security-critical line, not a style choice.
- **No plaintext secrets anywhere.** PINs and the admin secret exist in
  plaintext only while a human types them (and in the one-time admin key
  printout). The database stores only Argon2id hashes; the export stores only
  ciphertext. The exported document never contains PIN hashes either!

### The envelope flow (DEK → KEK → wrapped key)

"Envelope encryption"  works like a safe with two keys:
the data is locked in an envelope with one key, and *that key* is locked in a
small box whose key you keep. Here are the actors:

- **DEK** (data encryption key): 256 random bits. Locks the JSON export.
  Random, not derived from anything a human knows.
- **KEK** (key encryption key): derived from `admin PIN : admin secret` with
  Argon2id and the stored salt. Locks the DEK.
- **wrapped DEK**: the DEK encrypted with the KEK, saved in `config`.

```
 WRITE PATH (after every change)
  database rows ──> JSON text
        JSON text ──AES-GCM, fresh nonce, DEK──> ciphertext ──> atm_export.json
        DEK ──AES-GCM, KEK──> wrapped DEK ──> stored in config table

 READ PATH (admin decrypts)
  PIN + secret ──Argon2id + salt──> KEK
  wrapped DEK ──AES-GCM decrypt with KEK──> DEK
  ciphertext ──AES-GCM decrypt with DEK──> readable JSON ──> .json copy
```

**Why the extra key?** Because rotating credentials becomes cheap. You throw
away the old KEK, derive a new one from the new credentials, and re-wrap just
the 32-byte DEK. The export itself does not change a single byte, and any older
export file remains decryptable by whoever still has the matching credentials.
Re-encrypting a large export on every rotation would be slow and would create
a window where the file and the keys disagree.

- The DEK lives in memory (`Mutex<Option<[u8; 32]>>` in `store/mod.rs`) only
  while the admin key file is readable. If the key file is missing, the app
  prints a warning once and skips writing exports — it never silently writes
  an export it cannot account for.
- The wrapped DEK and the salt are *not secret*; keeping them in SQLite is fine
  because without the PIN+secret there is no KEK, and without the KEK there is
  no DEK, and without the DEK there is no readable data. This chain of
  dependencies is the whole point of the design.

### Key rotation flow

```
1. Admin menu → "Rotate admin PIN & secret key" → confirm (y/N)
2. New random PIN + new random 24-char secret are generated
3. New salt + new KEK derived from them
4. SAME DEK re-wrapped with the new KEK
5. admin hashes, salt and wrapped DEK updated in ONE SQL transaction
6. New credentials printed ONCE and data/admin.key rewritten (mode 0600)
7. Export rewritten with the unchanged DEK (still valid)
```

**Why "shown once"?** The printout is the moment the human copies the
credentials into their password manager of choice. The app does not keep a
history of them. Old credentials stop working immediately (verified by tests
in `tests/admin_ops.rs`).

---

## 2. Threading and concurrency

Three cooperating mechanisms keep two users from corrupting each other's
work. Think of a single cashier's desk with a queue, a signed receipt book,
and a "do not disturb" sign while writing:

- **Thread per connection.** The TCP listener listens for newcomers; each
  accepted connection gets its own worker thread. The console user has the
  main thread. Every thread runs the *same* menu code because all three
  transports implement the `Session` trait (console, TCP, and the test mock).
  A slow or disconnected user only stalls their own thread.
- **One `Mutex` around the SQLite connection.** A mutex is a lock: only the
  thread holding it may use the database. All sessions share one `Store`, so
  operations are queued rather than interleaved. This is deliberately simple:
  a few ATM users never need a more complex scheme, and correctness is much
  easier to reason about.
- **Transactions for money.** Before any balance change the code starts a
  SQL `BEGIN … COMMIT` transaction. **Atomicity** means "all of it or none of
  it": the debit, the credit, and both ledger rows succeed together, or the
  whole operation is rolled back. The balance check happens *inside* that
  transaction — not before acquiring the lock — so two simultaneous
  withdrawals cannot both read the same starting balance and both succeed.
  This is the classic **lost update** bug, and the transaction prevents it.
- **Export lock.** After a commit, `write_export` takes its own lock before
  writing the JSON file, so two sessions cannot interleave a half-written
  file. A reader always sees one complete encrypted snapshot.


```
 thread A ─┐                         ┌─> mutex ─> BEGIN ─> UPDATE ─> COMMIT
 thread B ─┼─> shared Store (Arc) ───┤
 thread C ─┘                         └─> export lock ─> encrypt ─> write file
```

**What the tests prove:** 
- `tests/concurrency.rs` hammers the same accounts
from several threads — many small deposits all land; concurrent withdrawals
never overdraw; a race for the last available funds lets exactly one win;
and transfers between customers stay atomic. 
- `tests/seed_users.rs` repeats the
idea with freshly generated wizard users, proving they are first-class
customers with full parallel behaviour.

---

## 3. Lockout and admin security

### Why the strike counter lives in the database?

- Three wrong PIN attempts lock a customer. The naive implementation keeps a
  counter in a variable in memory — but a variable disappears when the
  program restarts, or when a TCP client reconnects. That would let an
  attacker get two strikes, hang up, and start over forever.
- Our counter (`failed_attempts`) is a column in SQLite. It survives restarts,
  and console and TCP sessions see the same value because they share the same
  database. On a successful login it is reset to zero; an admin unlock also
  resets it.
- The `locked` flag is checked *before* asking for a PIN, so a locked account
  cannot even try: it is told to contact the admin. This mirrors real ATM
  cards, which swallow the card after too many attempts.
- A similar rule protects admin mode: three wrong PIN+secret pairs end the
  login attempt and return to the main screen.

### Admin-only gates

- Admin mode requires **two** things: the PIN *and* a separate secret key.
  Knowing only one is not enough (a second factor in spirit, though both are
  still "something you know").
- Decrypting the export asks for the credentials again — viewing the
  plaintext copy is a privileged act, not something left available after
  a menu session drifts.
- The **seed-users wizard**  adds a third gate: after staging
  names, account counts and balances for a whole batch, the admin must
  re-enter PIN + secret before a single row is inserted. A wrong key discards
  the batch. This is checked in `tests/seed_users.rs`.
- Generated users get random 4-digit PINs. The PINs are hashed before being
  stored and printed **once** in the end-of-wizard report; there is no way to
  ask the app for them again afterwards (only to reset them as admin).

### What is deliberately *not* here

- No hardware security module: `data/admin.key` with mode `0600` is a stand-in
  for the vault a real bank would use. Losing it makes the encrypted export
  unreadable — by design, and the app warns about it.
- No audit-log server or network encryption beyond localhost: this is a
  learning project. Real deployments would add TLS, hardware-backed secrets,
  and integer-cent balances instead of floating-point numbers.



