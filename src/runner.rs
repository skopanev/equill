use crate::{command, dispatch, kernel, vector};

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
    progress: Option<&mut dyn vector::VectorProgressSink>,
) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    let cli = command::cli::Cli::parse_from(args);
    // Every command that opens a store gives a lagging index one catch-up
    // attempt. The worker itself is excluded so it cannot start a copy of its
    // own work, and read-held stores stay passive.
    if let Some(store) = cli.command.store_to_resume()
        && !command::cli::held_to_reading(store)
    {
        vector::resume(store);
    }
    dispatch(cli, progress)
}
