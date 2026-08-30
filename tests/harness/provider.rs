//! A provider the test controls, speaking enough HTTP/2 to hold a real gRPC
//! call open.
//!
//! Two earlier versions were not good enough, and both failures were the same
//! shape: the harness claimed more than it proved. The first held the TCP
//! connection without any handshake, so the client treated it as a connection
//! error and the worker exited in milliseconds. The second wrote a SETTINGS
//! frame and acknowledged whatever arrived without reading it, and counted a
//! raw TCP accept as "the worker reached the provider" — which a worker that
//! immediately failed would also satisfy.
//!
//! This one parses. It reads the client's connection preface, reads its frames,
//! answers SETTINGS properly, and counts a connection as REACHED only when the
//! client has actually sent HEADERS — a request on a stream. Then it answers
//! nothing, so that request stays pending for as long as the test needs.
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The 24-byte client connection preface, RFC 9113 section 3.4.
pub(super) const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
/// Frame types this needs to recognise.
pub(super) const SETTINGS: u8 = 0x4;
pub(super) const HEADERS: u8 = 0x1;
/// An empty SETTINGS frame, and the same with the ACK flag.
pub(super) const SETTINGS_FRAME: [u8; 9] = [0, 0, 0, SETTINGS, 0, 0, 0, 0, 0];
pub(super) const SETTINGS_ACK: [u8; 9] = [0, 0, 0, SETTINGS, 1, 0, 0, 0, 0];

/// How far the last connection got through the handshake. Diagnostic only: when
/// a test fails on "never sent a request", this says whether the client never
/// arrived, stalled at the preface, or was cut off mid-frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    NoConnection,
    Connected,
    PrefaceRead,
    SettingsExchanged,
    RequestReceived,
    /// The client vanished before its preface was complete.
    AbortedAtPreface,
    /// The connection ended between frames, after the preface.
    AbortedBetweenFrames,
    /// Writing our SETTINGS or its acknowledgement failed.
    AbortedOnWrite,
}

pub struct SlowProvider {
    port: u16,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    stage: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
    held: Arc<Mutex<Vec<TcpStream>>>,
}

impl SlowProvider {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        // Blocking accept. The non-blocking poll slept 5ms between attempts,
        // which on a loaded machine became tens of milliseconds before the
        // connection was even accepted — long enough for the client to give up
        // on the connection before the handshake began. Shutdown wakes this by
        // connecting to itself.
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(AtomicUsize::new(0));
        let held: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let thread = {
            let stop = Arc::clone(&stop);
            let requests = Arc::clone(&requests);
            let stage = Arc::clone(&stage);
            let held = Arc::clone(&held);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            let requests = Arc::clone(&requests);
                            let stage = Arc::clone(&stage);
                            let held = Arc::clone(&held);
                            std::thread::spawn(move || {
                                super::h2::serve(stream, requests, stage, held)
                            });
                        }
                        Err(_) => return,
                    }
                }
            })
        };
        Self {
            port,
            stop,
            requests,
            stage,
            thread: Some(thread),
            held,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// How many gRPC requests are being held open.
    ///
    /// Counted at HEADERS, not at accept: a worker that connected and then died
    /// never sends one, so this cannot be satisfied by a failure.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }

    /// How far the handshake got. Read when a test fails, so the failure names
    /// the stage rather than only its symptom.
    pub fn stage(&self) -> Stage {
        match self.stage.load(Ordering::Relaxed) {
            1 => Stage::Connected,
            2 => Stage::PrefaceRead,
            3 => Stage::SettingsExchanged,
            4 => Stage::RequestReceived,
            5 => Stage::AbortedAtPreface,
            6 => Stage::AbortedBetweenFrames,
            7 => Stage::AbortedOnWrite,
            _ => Stage::NoConnection,
        }
    }

    /// Let every pending call go, so the workers can finish.
    pub fn release(&self) {
        for stream in self.held.lock().expect("held").drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for SlowProvider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept so the thread can see the stop flag.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        self.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
