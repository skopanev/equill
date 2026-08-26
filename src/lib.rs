pub mod command;
pub mod defense;
pub mod ingest;
pub mod integrity;
pub mod kernel;
pub mod projection;
pub mod record;
pub mod schema;

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
        } => {
            let report = command::init::create(&store, &owner, &namespace)?;
            command::output::render(json, &report, command::output::init(&store, &report))
        }
        command::cli::Command::Record { store, input } => {
            let actor = kernel::identity::actor_from_env()?;
            let report = record::append_file(&store, &input, &actor)?;
            command::output::render(json, &report, command::output::record(&report))
        }
        command::cli::Command::Import { store, input } => {
            let actor = kernel::identity::actor_from_env()?;
            let report = ingest::import_jsonl(&store, &input, &actor)?;
            command::output::render(json, &report, command::output::import(&report))
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
        } => {
            let request = projection::SearchRequest {
                query,
                namespace,
                type_name,
                limit,
            };
            let report = projection::search(&store, &request)?;
            command::output::render(json, &report, command::output::search(&report))
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
