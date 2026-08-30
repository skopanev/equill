//! Just enough HTTP/2 to complete a handshake and then hold a request open.
use super::provider::{HEADERS, PREFACE, SETTINGS, SETTINGS_ACK, SETTINGS_FRAME};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Read exactly `len` bytes, or give up.
fn read_exact(stream: &mut TcpStream, buffer: &mut [u8]) -> bool {
    stream.read_exact(buffer).is_ok()
}

pub(super) fn serve(
    mut stream: TcpStream,
    requests: Arc<AtomicUsize>,
    stage: Arc<AtomicUsize>,
    held: Arc<Mutex<Vec<TcpStream>>>,
) {
    stage.store(1, Ordering::Relaxed);
    let _ = stream.set_nodelay(true);
    // NO read timeout during the handshake. read_exact on a timing-out socket
    // reports an error AND discards whatever it had already buffered, so a
    // client that pauses between frames — which a real one does, since the
    // connection is established before the request is made — was being dropped
    // mid-handshake and never got to send HEADERS.

    // Our SETTINGS goes out immediately, before reading anything. A server may
    // send its connection preface as soon as the connection exists, and h2
    // clients wait for it: sending ours only after reading theirs left the
    // client waiting on us while we waited on it, and under load that race was
    // lost often enough to fail one run in five with the connection closed
    // between frames.
    if stream.write_all(&SETTINGS_FRAME).is_err() {
        stage.store(7, Ordering::Relaxed);
        return;
    }
    let _ = stream.flush();

    // The client's preface comes first and is fixed. Anything else is not an
    // HTTP/2 client and there is nothing to hold open.
    let mut preface = [0_u8; 24];
    if !read_exact(&mut stream, &mut preface) || preface != PREFACE {
        stage.store(5, Ordering::Relaxed);
        return;
    }
    stage.store(2, Ordering::Relaxed);

    let mut header = [0_u8; 9];
    loop {
        if !read_exact(&mut stream, &mut header) {
            stage.store(6, Ordering::Relaxed);
            return;
        }
        let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let kind = header[3];
        let flags = header[4];
        let mut payload = vec![0_u8; length];
        if length > 0 && !read_exact(&mut stream, &mut payload) {
            stage.store(6, Ordering::Relaxed);
            return;
        }
        match kind {
            // Acknowledge the client's settings; ignore its acknowledgement of
            // ours.
            SETTINGS if flags & 1 == 0 => {
                if stream.write_all(&SETTINGS_ACK).is_err() {
                    stage.store(7, Ordering::Relaxed);
                    return;
                }
                let _ = stream.flush();
                stage.store(3, Ordering::Relaxed);
            }
            // A request on a stream. This is the moment the worker is genuinely
            // waiting on the provider, so it is the moment worth counting — and
            // from here we answer nothing at all.
            HEADERS => {
                requests.fetch_add(1, Ordering::Relaxed);
                stage.store(4, Ordering::Relaxed);
                if let Ok(copy) = stream.try_clone() {
                    held.lock().expect("held").push(copy);
                }
                hold(stream, held);
                return;
            }
            _ => {}
        }
    }
}

/// Keep the connection open until the test releases it. Nothing is written, so
/// the call above stays outstanding.
fn hold(mut stream: TcpStream, held: Arc<Mutex<Vec<TcpStream>>>) {
    let mut buffer = [0_u8; 1024];
    loop {
        if held.lock().expect("held").is_empty() {
            return;
        }
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}
