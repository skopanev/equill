//! One `equill mcp` process, held open for the length of a measurement.
//!
//! This is the path a real agent uses: a session is opened once and then serves
//! many calls. Timing the CLI instead charges every measurement for a fork, an
//! exec and a dynamic link — on this host most of the budget — which says more
//! about process startup than about the work.
//!
//! Finite by construction: the process is spawned by the test, exits when the
//! test drops it, and outlives nothing.
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    pub fn open(root: &Path) -> Self {
        let mut child = Command::new(super::binary())
            .args(["mcp"])
            .arg("--store")
            .arg(root)
            .env("EQUILL_ACTOR", "owner")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("mcp session");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut session = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        // Negotiate before anything is timed, so the handshake is not counted as
        // part of a call.
        session.call(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "equill-bench", "version": "0" }
            }),
        );
        session
    }

    /// One JSON-RPC round trip, timed at the boundary a caller sees.
    pub fn timed(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> (Duration, serde_json::Value) {
        let started = Instant::now();
        let response = self.call(method, params);
        (started.elapsed(), response)
    }

    pub fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        writeln!(self.stdin, "{request}").expect("write request");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read response");
        serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!("unreadable response to {method}: {error}: {line}");
        })
    }

    /// A tool call, which is what the write and read measurements actually make.
    pub fn tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> (Duration, serde_json::Value) {
        self.timed(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
