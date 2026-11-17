use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

// Why a trait object (`&mut dyn Session`) instead of an enum or generics:
// the menu code in `app.rs`, `operations/` and `admin/` should not care where
// input comes from. Console, TCP and the in-memory mock used by tests all
// implement this one trait, so there is exactly ONE copy of the menu logic
// for three transports. The `prompt` helper (print → flush → read a line)
// is shared so every transport has identical prompt behaviour — including
// flushing before blocking on input, which is what makes prompts visible
// over a socket and in a pipe.
pub trait Session {
    fn print(&mut self, s: &str) -> io::Result<()>;
    fn read_line(&mut self) -> io::Result<String>;
    fn flush(&mut self) -> io::Result<()>;

    fn clear(&mut self) -> io::Result<()> {
        self.print("\x1B[2J\x1B[1;1H")
    }

    fn prompt(&mut self, msg: &str) -> io::Result<String> {
        self.print(msg)?;
        self.flush()?;
        Ok(self.read_line()?.trim().to_string())
    }
}

pub struct ConsoleSession;

impl Session for ConsoleSession {
    fn print(&mut self, s: &str) -> io::Result<()> {
        print!("{s}");
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }

    fn read_line(&mut self) -> io::Result<String> {
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)?;
        // Why EOF is an error here: `app.rs` treats UnexpectedEof as a clean
        // end-of-session. This is what makes a closed pipe (e.g. a piped test
        // script finishing, or the user pressing Ctrl-D) exit quietly
        // instead of looping forever on empty input.
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "input closed"));
        }
        Ok(line)
    }
}

pub struct TcpSession {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

// MockSession exists for automated tests: it behaves like a user sitting at
// the terminal. Pre-loaded input lines are returned one by one from
// read_line(), and everything the app "prints" is collected in `output` so a
// test can assert on messages like "[OK] Deposited 50.00 EUR".
// When the scripted input runs out, read_line() returns UnexpectedEof — the
// same signal a real terminal or disconnected TCP client produces — which
// cleanly ends the session loop.
pub struct MockSession {
    lines: std::collections::VecDeque<String>,
    output: String,
}

impl MockSession {
    pub fn new(input: &[&str]) -> Self {
        MockSession {
            lines: input.iter().map(|s| s.to_string()).collect(),
            output: String::new(),
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }
}

impl Session for MockSession {
    fn print(&mut self, s: &str) -> io::Result<()> {
        self.output.push_str(s);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn read_line(&mut self) -> io::Result<String> {
        self.lines
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "scripted input exhausted"))
    }
}

impl TcpSession {
    pub fn new(stream: TcpStream) -> io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(TcpSession {
            reader: BufReader::new(stream),
            writer,
        })
    }
}

impl Session for TcpSession {
    fn print(&mut self, s: &str) -> io::Result<()> {
        self.writer.write_all(s.as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    fn read_line(&mut self) -> io::Result<String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "client disconnected"));
        }
        Ok(line)
    }
}
