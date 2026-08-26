pub mod command;
pub mod defense;
pub mod integrity;
pub mod kernel;
pub mod record;
pub mod schema;

pub fn run<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    let cli = command::cli::Cli::parse_from(args);
    match cli.command {
        command::cli::Command::Init {
            store,
            owner,
            namespace,
        } => Ok(serde_json::to_string(&command::init::create(
            &store, &owner, &namespace,
        )?)?),
        command::cli::Command::Record { store, input } => {
            let actor = kernel::identity::actor_from_env()?;
            Ok(serde_json::to_string(&record::append_file(
                &store, &input, &actor,
            )?)?)
        }
        command::cli::Command::Doctor { store, full, deep } => Ok(serde_json::to_string(
            &command::doctor::report(store.as_deref(), full, deep)?,
        )?),
        command::cli::Command::Schema { command } => {
            let actor = kernel::identity::actor_from_env()?;
            match command {
                command::cli::SchemaCommand::Register { store, file } => Ok(serde_json::to_string(
                    &schema::register_file(&store, &file, &actor)?,
                )?),
            }
        }
        command::cli::Command::Status { store } => Ok(serde_json::to_string(
            &command::status::report(store.as_deref())?,
        )?),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn doctor_is_machine_readable() {
        let output = super::run(["equill", "doctor"]).expect("doctor output");
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(value["ok"], true);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }
}
