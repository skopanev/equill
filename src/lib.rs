pub mod command;
pub mod compact;
pub mod context;
pub mod defense;
pub mod filter;
pub mod ingest;
pub mod integrity;
pub mod kernel;
pub mod projection;
pub mod record;
pub mod schema;
pub mod vector;

pub fn run<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    let cli = command::cli::Cli::parse_from(args);
    let json = cli.json;
    match cli.command {
        command::cli::Command::Init {
            store,
            owner,
            namespace,
            writers,
        } => {
            let report = command::init::create_with_writers(&store, &owner, &namespace, &writers)?;
            command::output::render(json, &report, command::output::init(&store, &report))
        }
        command::cli::Command::Record { store, input } => {
            let actor = kernel::identity::actor_from_env()?;
            let report = record::append_file(&store, &input, &actor)?;
            command::output::render(json, &report, command::output::record(&report))
        }
        command::cli::Command::Import {
            store,
            input,
            manifest,
        } => {
            let actor = kernel::identity::actor_from_env()?;
            if let Some(input) = input {
                let report = ingest::import_jsonl(&store, &input, &actor)?;
                command::output::render(json, &report, command::output::import(&report))
            } else {
                let manifest = manifest.expect("clap requires one import input");
                let report = ingest::import_manifest(&store, &manifest, &actor)?;
                command::output::render(json, &report, command::output::import_set(&report))
            }
        }
        command::cli::Command::Compact {
            store,
            manifest,
            dry_run,
            apply,
        } => {
            let actor = kernel::identity::actor_from_env()?;
            let report = compact::run(&store, &manifest, apply && !dry_run, &actor)?;
            command::output::render(json, &report, command::output::compact(&report))
        }
        command::cli::Command::Doctor { store, full, deep } => {
            let report = command::doctor::report(store.as_deref(), full, deep)?;
            command::output::render(json, &report, command::output::doctor(&report))
        }
        command::cli::Command::Schema { command } => {
            let actor = kernel::identity::actor_from_env()?;
            match command {
                command::cli::SchemaCommand::Register { store, file } => {
                    let report = schema::register_file(&store, &file, &actor)?;
                    command::output::render(json, &report, command::output::schema(&report))
                }
            }
        }
        command::cli::Command::Profile { command } => {
            let actor = kernel::identity::actor_from_env()?;
            match command {
                command::cli::RegistryCommand::Register { store, file } => {
                    let report = context::register_profile(&store, &file, &actor)?;
                    command::output::render(
                        json,
                        &report,
                        command::output::registry("profile", &report),
                    )
                }
            }
        }
        command::cli::Command::Selector { command } => {
            let actor = kernel::identity::actor_from_env()?;
            match command {
                command::cli::RegistryCommand::Register { store, file } => {
                    let report = context::register_selector(&store, &file, &actor)?;
                    command::output::render(
                        json,
                        &report,
                        command::output::registry("selector", &report),
                    )
                }
            }
        }
        command::cli::Command::Context {
            store,
            profile,
            request,
            query,
            coordinates,
            tags,
            kinds,
            at,
            filters,
            strict,
            format,
            fields,
        } => {
            let actor = kernel::identity::actor_from_env()?;
            let filter = filter::Filter::parse(&filters, strict)?;
            let bundle = match request {
                Some(path) => context::assemble_file(&store, &profile, &path, &actor, &filter)?,
                None => {
                    let request = context::inline_request(query, coordinates, tags, kinds, at)?;
                    context::assemble(&store, &profile, request, &actor, &filter)?
                }
            };
            let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
                bundle.content.clone()
            } else {
                let selected = record::read_all(&store)?
                    .into_iter()
                    .filter(|item| bundle.selected_record_ids.contains(&item.id))
                    .collect::<Vec<_>>();
                command::present::records(&selected, shape(format), &fields)?
            };
            command::output::render(json, &bundle, text)
        }
        command::cli::Command::Status { store } => {
            let report = command::status::report(store.as_deref())?;
            command::output::render(json, &report, command::output::status(&report))
        }
        command::cli::Command::Search {
            store,
            query,
            namespace,
            type_name,
            limit,
            strategy,
            filters,
            strict,
            format,
            fields,
        } => {
            let filter = filter::Filter::parse(&filters, strict)?;
            filter::validate(&filter, &filter::in_scope(&store, type_name.as_deref())?)?;
            // The projection caps its own result set, so a filter that runs
            // afterwards must inspect the entire corpus or refuse explicitly.
            let pool = if filter.is_empty() {
                limit
            } else {
                filter::candidate_limit(record::read_all(&store)?.len(), limit)?
            };
            let request = projection::SearchRequest {
                query,
                namespace,
                type_name,
                limit: pool,
            };
            let strategy = match strategy {
                command::cli::StrategyArg::Fts => vector::SearchStrategy::Fts,
                command::cli::StrategyArg::Vector => vector::SearchStrategy::Vector,
                command::cli::StrategyArg::Hybrid => vector::SearchStrategy::Hybrid,
            };
            let mut report = vector::search(&store, &request, strategy)?;
            report
                .hits
                .retain(|hit| filter::matches(&hit.record.payload, &filter));
            report.hits.truncate(limit as usize);
            let text = match &report.fallback {
                Some(reason) => format!(
                    "{} hits via {} (vector unavailable: {reason})",
                    report.hits.len(),
                    report.answered_by
                ),
                None => format!("{} hits via {}", report.hits.len(), report.answered_by),
            };
            let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
                text
            } else {
                let hits = report
                    .hits
                    .iter()
                    .map(|hit| hit.record.clone())
                    .collect::<Vec<_>>();
                command::present::records(&hits, shape(format), &fields)?
            };
            command::output::render(json, &report, text)
        }
        command::cli::Command::Vector { command } => {
            let actor = kernel::identity::actor_from_env()?;
            match command {
                command::cli::VectorCommand::Configure { store, file } => {
                    let report = vector::configure(&store, &file, &actor)?;
                    let text = format!(
                        "Vector projection configured — alias {} ({})",
                        report.collection_alias,
                        if report.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    command::output::render(json, &report, text)
                }
                command::cli::VectorCommand::Disable { store } => {
                    let report = vector::disable(&store, &actor)?;
                    command::output::render(json, &report, "Vector projection disabled".into())
                }
                command::cli::VectorCommand::Rebuild { store } => {
                    let report = vector::rebuild(&store, &actor)?;
                    let text = format!(
                        "Vector projection rebuilt — {} records into {}",
                        report.records, report.collection
                    );
                    command::output::render(json, &report, text)
                }
            }
        }
        command::cli::Command::Rebuild { store } => {
            let report = projection::rebuild(&store)?;
            command::output::render(json, &report, command::output::rebuild(&report))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn doctor_is_machine_readable() {
        let output = super::run(["equill", "doctor", "--json"]).expect("doctor output");
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(value["ok"], true);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));

        let human = super::run(["equill", "doctor"]).expect("human doctor output");
        assert!(human.starts_with("Equill doctor (quick) — OK"));
    }
}

fn shape(format: command::cli::FormatArg) -> command::present::Format {
    match format {
        command::cli::FormatArg::Jsonl => command::present::Format::Jsonl,
        command::cli::FormatArg::Text => command::present::Format::Text,
    }
}
