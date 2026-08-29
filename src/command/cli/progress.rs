use crate::vector::{VectorProgress, VectorProgressSink};
use std::io::{self, Write};
use std::time::{Duration, Instant};

pub(crate) struct HumanVectorProgress<W> {
    writer: W,
    embedding_started: Option<Instant>,
}

impl HumanVectorProgress<io::Stderr> {
    pub(crate) fn stderr() -> Self {
        Self::new(io::stderr())
    }
}

impl<W: Write> HumanVectorProgress<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            embedding_started: None,
        }
    }
}

impl<W: Write> VectorProgressSink for HumanVectorProgress<W> {
    fn emit(&mut self, event: VectorProgress) {
        if event == VectorProgress::LoadingModel {
            self.embedding_started = Some(Instant::now());
        }
        let elapsed = self.embedding_started.map_or(Duration::ZERO, |started| {
            Instant::now().saturating_duration_since(started)
        });
        let _ = writeln!(self.writer, "{}", format_event(&event, elapsed));
    }
}

fn format_event(event: &VectorProgress, elapsed: Duration) -> String {
    match event {
        VectorProgress::Connecting { collection } => {
            format!("vector: connecting — collection {collection}")
        }
        VectorProgress::LoadingModel => "vector: loading model".into(),
        VectorProgress::Scanned {
            collection,
            records,
            pending,
            corpus_sha256,
        } => format!(
            "vector: scanned {records} records — {pending} pending — corpus {corpus_sha256} — collection {collection}"
        ),
        VectorProgress::Embedded { completed, total } => {
            let percent = percentage(*completed, *total);
            let eta = eta_seconds(*completed, *total, elapsed);
            format!("vector: embedded {completed}/{total} ({percent}%) — ETA {eta}s")
        }
        VectorProgress::Upserted { completed, total } => {
            format!("vector: upserted {completed}/{total}")
        }
        VectorProgress::Ready {
            collection,
            corpus_sha256,
        } => format!("vector: ready — collection {collection} — corpus {corpus_sha256}"),
    }
}

fn percentage(completed: usize, total: usize) -> usize {
    completed
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(100)
}

fn eta_seconds(completed: usize, total: usize, elapsed: Duration) -> u64 {
    if completed == 0 || completed >= total {
        return 0;
    }
    let remaining = total - completed;
    (elapsed.as_secs_f64() * remaining as f64 / completed as f64).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deterministic_lines_use_only_safe_progress_coordinates() {
        let hash = "a".repeat(64);
        let event = VectorProgress::Scanned {
            collection: "equill_records_test".into(),
            records: 10,
            pending: 2,
            corpus_sha256: hash.clone(),
        };
        assert_eq!(
            format_event(&event, Duration::ZERO),
            format!(
                "vector: scanned 10 records — 2 pending — corpus {hash} — collection equill_records_test"
            )
        );
        assert_eq!(
            format_event(
                &VectorProgress::Embedded {
                    completed: 2,
                    total: 10,
                },
                Duration::from_millis(2500),
            ),
            "vector: embedded 2/10 (20%) — ETA 10s"
        );
    }

    #[test]
    fn progress_writer_does_not_change_json_stdout() {
        let mut sink = HumanVectorProgress::new(Vec::new());
        sink.emit(VectorProgress::LoadingModel);
        let stdout = crate::command::output::render(true, &json!({"ok": true}), "human".into())
            .expect("JSON output");

        assert_eq!(stdout, r#"{"ok":true}"#);
        assert_eq!(
            String::from_utf8(sink.writer).unwrap(),
            "vector: loading model\n"
        );
    }
}
