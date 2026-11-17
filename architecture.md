# Architecture

It shows how the pieces fit together and **why** they were chosen. You do not need to know Rust to
follow most of it. For the close-up view see
[details.md](details.md).

---

## 1. The overall shape: several users, one ATM

The app is a **client–server** system. One central program
(*server*) owns the money data, and several user programs (*clients*)
connect to it to ask questions and make changes. A client can be the terminal
window on the same machine (the console) or a remote terminal connected over
TCP (the network equivalent of a phone line to the bank).

![Client–server model — clients talking to one server](https://upload.wikimedia.org/wikipedia/commons/b/bb/Client-Server_Model-en.svg)

[Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Client-Server_Model-en.svg)


```
 [Console user]          [Remote user A]        [Remote user B]
      |                         |                      |
      | stdin/stdout            | TCP 127.0.0.1:8080   | TCP ...
      +------------\     /-------+--------------/-------+
                    v   v
              +--------------------+
              |   ONE ATM process  |
              |  shared Store      |
              +----------+---------+
                         |
                  +------v------+
                  | data/atm.db |
                  +-------------+
```

**Why client–server at all?** 
A real ATM must let more than one person use it at a time and must keep a
single, consistent source of truth for balances. Centralising the data in one
process is what makes "two people, same account, no lost money" possible.

---

## 2. Layered design

The code is arranged in **layers** — like a restaurant: the waiter takes your
order (input), the kitchen prepares it (business logic), and the storeroom
holds the ingredients (data storage). A layer only talks to the layer next to
it, which keeps the code easy to change.

![Three-tier application overview](https://upload.wikimedia.org/wikipedia/commons/5/51/Overview_of_a_three-tier_application_vectorVersion.svg)

*[Wikimedia Commons](https://upload.wikimedia.org/wiki/File:Overview_of_a_three-tier_application_vectorVersion.svg)


```
┌─────────────────────────────────────────────────────────┐
│ 1. PRESENTATION   main.rs / app.rs / session.rs         │
│    menus, prompts, console + TCP + mock sessions        │
├─────────────────────────────────────────────────────────┤
│ 2. OPERATIONS     auth.rs / admin/ / operations/        │
│    login, lockout, customer menu, admin menu, wizard    │
├─────────────────────────────────────────────────────────┤
│ 3. STORAGE        store/  (SQLite, mutex, transactions) │
│    schema, accounts, money, customers, admin, export    │
├─────────────────────────────────────────────────────────┤
│ 4. SIDE EFFECT    data/atm_export.json (encrypted)      │
└─────────────────────────────────────────────────────────┘
```

### Module map

| Area | Files | Responsibility |
| --- | --- | --- |
| Entry / transport | `main.rs`, `app.rs`, `session.rs` | Open the store, start TCP listener, run the console session, share one menu loop across transports |
| Customer logic | `auth.rs`, `operations/mod.rs`, `operations/account_ops.rs`, `operations/money_ops.rs`, `operations/pin_ops.rs` | Login + lockout; customer menu; accounts, money movement, statements, PIN changes |
| Admin logic | `admin/mod.rs`, `admin/user_mgmt.rs`, `admin/maintenance.rs` | Admin login/menu; customer management; transactions, decrypt, rotation, seed wizard |
| Storage | `store/mod.rs`, `store/error.rs`, `store/schema.rs`, `store/accounts.rs`, `store/money.rs`, `store/customers.rs`, `store/admin_ops.rs`, `store/export.rs` | One SQLite connection, schema/seeding, all CRUD, credentials, encrypted snapshot |
| Support | `crypto.rs`, `models.rs`, `utils.rs` | Hashing/encryption, data types, input validation |


---

## 3. Data flow

A single action travels through every layer. Example — a user deposits 50 EUR:

```
1. User types "f" then "50"
        │
        ▼
2. Session.read_line()          (console stdin OR TCP socket; same trait)
        │
        ▼
3. operations/money_ops.rs      parse amount, call store.deposit(...)
        │
        ▼
4. store/money.rs               lock the mutex → BEGIN transaction
        │                       UPDATE balance → INSERT ledger row → COMMIT
        ▼
5. data/atm.db                  durable, correct balance + history
        │
        ▼
6. store/export.rs              AFTER the commit: reload everything,
        │                       encrypt with the DEK, write the JSON snapshot
        ▼
7. data/atm_export.json         always current, always ciphertext
```

**Why this order matters:** the database is the *source of truth*; the JSON
file is an *export/backup*. By writing the export only **after** a successful
database commit, a crash can never leave a stale export pretending to be
authoritative. If the export write itself fails, the user still sees success
and a warning is printed — the money change already happened safely.

For admin actions the flow is the same, except step 3 runs in `admin/…` and
some operations (decrypt export, rotate credentials, seed users) add a
**re-authentication gate** before step 4.

### The seed flow

```
[INFO] how many users? → N → confirm once
   loop N times: generated name shown → admin enters account count and
   exact money-per-account → "Add this user?" (y/N) → running summary
        │
        ▼
final gate: re-enter admin PIN + secret key
        │  wrong → discard everything, no writes
        ▼
one SQL transaction inserts ALL staged users/accounts, each with a random PIN
        │
        ▼
one-time report (names, account numbers, balances, PINs) + export refresh
```

**Why the gate?** Staging data in memory is harmless; inserting it is not.
Requiring the credentials again right before the insert means a user who
walked away from an unlocked admin session cannot commit a batch of accounts.

---

## 4. Threading topology: one process, many conversations

The operating system concept used here is a **thread**: a small worker inside
one process that can run independently. We use **one thread per connected
user** (*thread-per-connection* model):


- `main.rs` opens one `Store`.
- `app.rs` starts a TCP listener thread; every accepted connection gets its
  own worker thread with its own `Session` object.
- The terminal user runs on the main thread with a `ConsoleSession`.
- All of them share the same store via `Arc` (an atomically reference-counted
  pointer: "several owners, one object").

![SQLite architecture overview](https://upload.wikimedia.org/wikipedia/commons/1/1b/SQLite_Architecture.svg)

 [official SQLite documentation](https://www.sqlite.org/arch.html)
 [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:SQLite_Architecture.svg)



```
  thread: console      thread: TCP #1           thread: TCP #2
  (main)               (spawned)               (spawned)
     \                     |                        /
      \                    |                       /
       +-------->  Mutex<SQLite Connection>  <----+
                          |
                    BEGIN ... COMMIT
                          |
                      data/atm.db
```

**Why a `Mutex`?** A mutex is a lock that only one thread can hold at a time.
The SQLite connection object does not like being used simultaneously, so the
mutex queues the threads: one operation at a time, no corrupted data. Because
ATM operations are short, this "one at a time" rule is more than fast enough.
Crucially, a money operation holds the lock from the balance check through the
commit, which is what prevents two withdrawals from both spending the same
money (a *lost update*).

**Why threads instead of async?** Each session is mostly idle, waiting for a
human to type. Threads make that simple to express: blocking on input blocks
only that one user's thread. For a handful of users, this is simpler and more
predictable than an asynchronous runtime.

**Why `dyn Session`?** The trait object means "any object that can print,
read a line and flush". The menu code is written once against this interface
and works for console, TCP and the mock session used by tests. One menu, three
transports, no copy-paste.

---

## 5. Key hierarchy

Nothing secret is stored in plain text in the database. There are two keys:

```
   admin PIN + secret key  ──Argon2id + stored salt──>  KEK  (key-encryption key)
                                                         │
                                                         │   ---wraps (encrypts)
                                                         v
                                                        DEK  (random, 256-bit)
                                                         │
                                                         │   ---encrypts
                                                         v
                                              JSON snapshot on disk
```

- The **DEK** is a random key that encrypts the JSON export. It never comes
  from a password.
- The **KEK** is derived from what the admin knows (PIN + secret) whenever the
  export must be decrypted, or when credentials are rotated.
- What is saved in the database is only the *wrapped* DEK (the DEK encrypted
  with the KEK). Losing the database does not reveal the export key; losing
  `data/admin.key` (the one-time printout of the credentials) makes the export
  unreadable — by design.

**Why this two-key arrangement?** Rotating the admin credentials only needs to
re-wrap a tiny 32-byte key with a new KEK. The (potentially large) export does
not have to be decrypted and re-encrypted, and old encrypted snapshots remain
readable with the matching credentials. This is called **envelope encryption**:
the DEK is the key to the data "envelope"; the KEK is the key to the box that
holds the DEK.

On first run, `data/admin.key` is created with mode `0600` (only the owner can
read/write it) and gitignored. It stands in for what a real bank would keep
in an HSM (a dedicated hardware secret vault).

![What is HSM?](https://thumb.wikimedia.org/wikipedia/commons/thumb/9/9b/NCipher_nShield_F3_Hardware_Security_Module.jpg/1920px-NCipher_nShield_F3_Hardware_Security_Module.jpg?utm_source=en.wikipedia.org&utm_campaign=imageinfo&utm_content=thumbnail)


---

## 6. Design choices 

| Choice | Why it was chosen |
| --- | --- |
| SQLite over in-memory data | Balances and history survive restarts; transactions give atomicity for free |
| Encrypted JSON export alongside it | A portable, auditable snapshot — unusable to anyone without the admin key |
| Argon2id for PINs/secrets | Memory-hard: expensive to brute-force, even with specialised hardware |
| AES-256-GCM for export | Confidentiality *and* tamper detection in one step |
| Lockout counters in the database | Restarting the app must not erase failed-attempt strikes |
| One mutex + transactions | Simple, correct concurrency for a small number of users |


> Further References:
https://en.wikipedia.org/wiki/Argon2)
https://en.wikipedia.org/wiki/Galois/Counter_Mode
