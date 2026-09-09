use crate::audit::{self, Scope};
use crate::kernel::error::Error;
use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Select bounded, payload-free events from the isolated request log.
    List {
        #[command(flatten)]
        scope: AuditScope,
        /// Maximum events, newest first.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=1000))]
        limit: u16,
    },
    /// Count requests and summarize durations over the identical filter scope.
    Stats {
        #[command(flatten)]
        scope: AuditScope,
    },
}

#[derive(Debug, Args)]
pub struct AuditScope {
    /// Inclusive RFC3339 event time.
    #[arg(long)]
    since: Option<String>,
    /// Exclusive RFC3339 event time.
    #[arg(long)]
    until: Option<String>,
    #[arg(long)]
    surface: Option<String>,
    #[arg(long)]
    operation: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    role: Option<String>,
    #[arg(long)]
    process: Option<String>,
    #[arg(long)]
    outcome: Option<String>,
    #[arg(long)]
    instance: Option<String>,
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    lane: Option<String>,
}

impl AuditScope {
    fn into_scope(self) -> Scope {
        Scope {
            since: self.since,
            until: self.until,
            surface: self.surface,
            operation: self.operation,
            project: self.project,
            role: self.role,
            process: self.process,
            outcome: self.outcome,
            instance: self.instance,
            session: self.session,
            actor: self.actor,
            lane: self.lane,
        }
    }
}

impl AuditCommand {
    pub(crate) fn run(self, json: bool) -> Result<String, Error> {
        match self {
            Self::List { scope, limit } => {
                let report = audit::list(&scope.into_scope(), limit)?;
                let text = report.events.iter().map(|event| format!(
                    "{} {} {} {} {} {}us project={} role={} process={} actor={} lane={} instance={} session={}",
                    event.observed_at, event.id, event.surface, event.operation,
                    event.outcome, event.duration_us,
                    event.project.as_deref().unwrap_or("-"), event.role.as_deref().unwrap_or("-"),
                    event.process.as_deref().unwrap_or("-"), event.actor_claimed.as_deref().unwrap_or("-"),
                    event.lane_claimed.as_deref().unwrap_or("-"), event.instance.as_deref().unwrap_or("-"),
                    event.session.as_deref().unwrap_or("-"),
                )).collect::<Vec<_>>().join("\n");
                crate::command::output::render(json, &report, text)
            }
            Self::Stats { scope } => {
                let report = audit::stats(&scope.into_scope())?;
                let text = format!(
                    "{} requests, {} failures ({:.2}%)\nduration_us min={} mean={:.2} p95={} max={}",
                    report.count,
                    report.failures,
                    report.error_rate * 100.0,
                    report.duration_min_us,
                    report.duration_mean_us,
                    report.duration_p95_us,
                    report.duration_max_us,
                );
                crate::command::output::render(json, &report, text)
            }
        }
    }
}
