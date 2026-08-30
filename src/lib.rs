pub mod command;
pub mod compact;
pub mod context;
pub mod defense;
pub mod filter;
pub mod governance;
pub mod ingest;
pub mod integrity;
pub mod kernel;
pub mod mcp;
pub mod projection;
pub mod record;
pub mod schema;
pub mod telemetry;
pub mod vector;

pub fn run<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    run_with_progress(args, None)
}

pub fn run_cli<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let mut progress = command::cli::HumanVectorProgress::stderr();
    run_with_progress(args, Some(&mut progress))
}

fn run_with_progress<I, T>(
    args: I,
    mut progress: Option<&mut dyn vector::VectorProgressSink>,
) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    let cli = command::cli::Cli::parse_from(args);
    let json = cli.json;
    if json {
        progress = None;
    }
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
            if record::is_batch(&input)? {
                let report = record::append_batch(&store, &input, &actor)?;
                let text = format!("{} stored, {} rejected", report.stored, report.rejected);
                return command::output::render(json, &report, text);
            }
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
        command::cli::Command::Schema { command } => match command {
            command::cli::SchemaCommand::List { store } => command::catalog::list(json, &store),
            command::cli::SchemaCommand::Show { store, type_name } => {
                command::catalog::show(json, &store, &type_name)
            }
            command::cli::SchemaCommand::Register { store, file } => {
                let actor = kernel::identity::actor_from_env()?;
                let report = schema::register_file(&store, &file, &actor)?;
                command::output::render(json, &report, command::output::schema(&report))
            }
        },
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
            project,
            role,
            phase,
            harness,
            tags,
            kinds,
            at,
            include_superseded,
            present,
        } => command::query::context(
            json,
            store,
            profile,
            request,
            query,
            coordinates,
            project,
            role,
            phase,
            harness,
            tags,
            kinds,
            at,
            include_superseded,
            present.filters,
            present.strict,
            present.format,
            present.fields,
        ),
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
            present,
            all,
        } => command::query::search(
            json,
            store,
            query,
            namespace,
            type_name,
            limit,
            strategy,
            present.filters,
            present.strict,
            present.format,
            present.fields,
            all,
        ),
        command::cli::Command::Vector { command } => {
            let actor = kernel::identity::actor_from_env()?;
            command::vector::run(json, command, &actor, progress)
        }
        command::cli::Command::Get {
            store,
            id,
            format,
            fields,
        } => command::query::get(json, &store, &id, format, &fields),
        command::cli::Command::Revoke { store, id, comment } => {
            let actor = kernel::identity::actor_from_env()?;
            let id: uuid::Uuid = id
                .parse()
                .map_err(|_| kernel::error::Error::InvalidRecord(format!("{id} is not an id")))?;
            let report = record::revoke(&store, id, comment.as_deref(), &actor)?;
            let text = format!(
                "Revoked {} — tombstone {}",
                report.revoked, report.tombstone
            );
            command::output::render(json, &report, text)
        }
        command::cli::Command::Mcp { store } => {
            let actor = kernel::identity::actor_from_env()?;
            let input = std::io::stdin().lock();
            let out = std::io::stdout().lock();
            mcp::serve(&store, &actor, telemetry::enabled(), input, out)?;
            Ok(String::new())
        }
        command::cli::Command::Owner { command } => command::authority::owner(json, command),
        command::cli::Command::Grant { command } => command::authority::grant(json, command),
        command::cli::Command::Rebuild { store } => {
            let report = projection::rebuild(&store)?;
            command::output::render(json, &report, command::output::rebuild(&report))
        }
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
