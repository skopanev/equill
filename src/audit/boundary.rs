use super::{Event, capture, writer};
use crate::kernel::error::Error;
use serde_json::Value;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

pub fn destination() -> Result<PathBuf, Error> {
    let path = match std::env::var_os("EQUILL_AUDIT_DIR") {
        Some(path) if path.is_empty() => return Err(writer::failure()),
        Some(path) => PathBuf::from(path),
        None => std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".equill/equill-log"))
            .ok_or_else(writer::failure)?,
    };
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}

pub struct Invocation {
    reservation: writer::Reservation,
    started: Instant,
    event: Event,
}

impl Invocation {
    fn new(
        surface: &str,
        operation: String,
        arguments: super::Arguments,
        decorate: impl FnOnce(&mut Event),
    ) -> Result<Self, Error> {
        static INSTANCE: OnceLock<String> = OnceLock::new();
        let started = Instant::now();
        let root = destination()?;
        let mut event = Event {
            schema_version: 1,
            id: uuid::Uuid::now_v7(),
            observed_at: jiff::Timestamp::now().to_string(),
            duration_us: 0,
            surface: surface.into(),
            operation,
            project: None,
            role: None,
            process: None,
            pid: std::process::id(),
            actor_claimed: None,
            lane_claimed: None,
            instance: None,
            session: None,
            outcome: "error".into(),
            domain_outcome: "unknown".into(),
            error_class: Some("interrupted".into()),
            arguments,
            output: capture::output(b""),
        };
        capture::coordinates(&mut event, |key| {
            std::env::var(format!("EQUILL_{}", key.to_ascii_uppercase())).ok()
        });
        let instance = INSTANCE.get_or_init(|| uuid::Uuid::now_v7().to_string());
        event.instance.get_or_insert_with(|| instance.clone());
        event.session.get_or_insert_with(|| instance.clone());
        event.process.get_or_insert_with(|| "equill".into());
        decorate(&mut event);
        let reservation =
            writer::Reservation::begin(&root, &event).map_err(|_| writer::failure())?;
        Ok(Self {
            reservation,
            started,
            event,
        })
    }

    pub fn cli(args: &[OsString]) -> Result<Option<Self>, Error> {
        let words: Vec<_> = args.iter().skip(1).filter_map(|s| s.to_str()).collect();
        let position = words.iter().position(|word| *word != "--json");
        let command = position
            .and_then(|index| words.get(index))
            .copied()
            .unwrap_or("invalid");
        // Audit reads and their validation errors never log themselves.
        if command == "audit" {
            return Ok(None);
        }
        let mut operation = capture::operation(command);
        if [
            "schema", "profile", "selector", "vector", "owner", "grant", "reader",
        ]
        .contains(&command)
            && let Some(subcommand) =
                position.and_then(|i| words[i + 1..].iter().find(|word| **word != "--json"))
        {
            operation.push('.');
            operation.push_str(&capture::operation(subcommand));
        }
        let invocation = Self::new("cli", operation, capture::cli(args), |event| {
            capture::overlay(event, |key| capture::cli_coordinate(&words, key));
        })?;
        Ok(Some(invocation))
    }

    pub fn mcp(bytes: &[u8], actor: &str) -> Result<Self, Error> {
        let request = serde_json::from_slice::<Value>(bytes).ok();
        let operation = request
            .as_ref()
            .and_then(|v| {
                if v["method"] == "tools/call" {
                    v.pointer("/params/name")
                } else {
                    v.get("method")
                }
            })
            .and_then(Value::as_str)
            .map(capture::operation)
            .unwrap_or_else(|| "invalid".into());
        let canonical = request.as_ref().and_then(|v| serde_json::to_vec(v).ok());
        let items = request
            .as_ref()
            .and_then(|v| v.pointer("/params/arguments"))
            .and_then(Value::as_object)
            .map_or(0, |v| v.len());
        Self::new(
            "mcp",
            operation,
            capture::arguments(canonical.as_deref().unwrap_or(bytes), items),
            |event| {
                event.actor_claimed = Some(capture::coordinate(actor));
                if let Some(arguments) = request
                    .as_ref()
                    .and_then(|v| v.pointer("/params/arguments"))
                {
                    capture::overlay(event, |key| capture::mcp_coordinate(arguments, key));
                }
            },
        )
    }

    pub fn prepare(&mut self, output: &[u8], error_class: Option<&str>) -> Result<(), Error> {
        self.event.duration_us = self
            .started
            .elapsed()
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX);
        self.event.output = capture::output(output);
        if self.event.surface == "cli" {
            super::result::apply(&mut self.event.output);
        }
        self.event.outcome = "success".into();
        self.event.domain_outcome = "success".into();
        self.event.error_class = None;
        if let Some(class) = error_class {
            self.event.outcome = "error".into();
            self.event.domain_outcome = "error".into();
            self.event.error_class = Some(capture::error_class(class).into());
        }
        self.reservation
            .checkpoint(&self.event)
            .map_err(|_| writer::failure())
    }

    pub fn settle(mut self, transport_failed: bool) -> Result<(), Error> {
        if transport_failed {
            self.event.outcome = "error".into();
            self.event.error_class = Some("transport".into());
        }
        self.reservation.finish(&self.event).map_err(|_| {
            Error::Audit(format!(
                "invocation {} completed but its request audit was not confirmed",
                self.event.id
            ))
        })
    }

    pub fn finish(mut self, output: &[u8], error_class: Option<&str>) -> Result<(), Error> {
        self.prepare(output, error_class)?;
        self.settle(false)
    }

    pub fn id(&self) -> uuid::Uuid {
        self.event.id
    }
}

pub(crate) fn error_class(error: &Error) -> &'static str {
    match error {
        Error::Io(_) => "io",
        Error::Json(_) | Error::Cli(_) => "validation",
        Error::MissingActor
        | Error::InvalidActor
        | Error::PermissionDenied
        | Error::ReadOnlyActor(_) => "authorization",
        Error::PostCommit(_) => "post_commit",
        Error::Audit(_) => "audit",
        Error::Integrity(_) => "integrity",
        _ => "execution",
    }
}
