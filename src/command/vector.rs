use super::cli::VectorCommand;
use crate::kernel::error::Error;
use crate::vector::{self, VectorProgressSink};

pub(crate) fn run(
    json: bool,
    command: VectorCommand,
    actor: &str,
    progress: Option<&mut dyn VectorProgressSink>,
) -> Result<String, Error> {
    match command {
        VectorCommand::Configure { store, file } => {
            let report = vector::configure(&store, &file, actor)?;
            let state = if report.enabled {
                "enabled"
            } else {
                "disabled"
            };
            super::output::render(
                json,
                &report,
                format!(
                    "Vector projection configured — alias {} ({state})",
                    report.collection_alias
                ),
            )
        }
        VectorCommand::Disable { store } => {
            let report = vector::disable(&store, actor)?;
            super::output::render(json, &report, "Vector projection disabled".into())
        }
        VectorCommand::Rebuild { store } => {
            let report = vector::rebuild_with_progress(&store, actor, progress)?;
            // The skipped count only when a filter is doing something: a
            // trailing "0 skipped" on every rebuild is noise that trains a
            // reader to stop looking at the line.
            let skipped = match report.records_skipped {
                0 => String::new(),
                count => format!(", {count} skipped by embed_types"),
            };
            let text = format!(
                "Vector projection rebuilt — {} records into {}{skipped}",
                report.records, report.collection
            );
            super::output::render(json, &report, text)
        }
        VectorCommand::Drain { store, once } => {
            if !once {
                return Err(Error::Projection(
                    "vector drain runs once; pass --once to say so explicitly".into(),
                ));
            }
            let report = vector::run_worker(&store)?;
            let text = format!("{} passes, {} embeddings", report.passes, report.embeddings);
            super::output::render(json, &report, text)
        }
        VectorCommand::Sync { store } => {
            let report = vector::sync_with_progress(&store, actor, progress)?;
            let text = format!(
                "Vector projection synced — {} embeddings, {} points upserted",
                report.embeddings, report.points_upserted
            );
            super::output::render(json, &report, text)
        }
    }
}
