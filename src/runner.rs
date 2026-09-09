use crate::{command, dispatch, kernel, vector};
use kernel::error::Error;
use std::io::Write;

type Delivery<'a> = Option<&'a mut dyn FnMut(&Result<String, Error>) -> std::io::Result<()>>;

pub fn run<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    run_with_progress(args, None, None)
}

pub fn run_cli<I, T>(args: I) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let mut progress = command::cli::HumanVectorProgress::stderr();
    run_with_progress(args, Some(&mut progress), None)
}

pub fn run_process<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let mut progress = command::cli::HumanVectorProgress::stderr();
    let mut deliver = |result: &Result<String, Error>| {
        let (body, stderr) = rendered(result);
        if stderr {
            std::io::stderr().write_all(body.as_bytes())?;
            std::io::stderr().flush()
        } else {
            std::io::stdout().write_all(body.as_bytes())?;
            std::io::stdout().flush()
        }
    };
    match run_with_progress(args, Some(&mut progress), Some(&mut deliver)) {
        Ok(_) => 0,
        Err(Error::Cli(error)) => error.exit_code(),
        Err(_) => 1,
    }
}

fn rendered(result: &Result<String, Error>) -> (String, bool) {
    match result {
        Ok(output) if output.is_empty() => (String::new(), false),
        Ok(output) => (format!("{output}\n"), false),
        Err(Error::Cli(error)) => (error.to_string(), error.use_stderr()),
        Err(error) if error.command_output().is_some() => (
            format!("{}\n", error.command_output().unwrap_or_default()),
            false,
        ),
        Err(error) => (format!("equill: {error}\n"), true),
    }
}

fn run_with_progress<I, T>(
    args: I,
    progress: Option<&mut dyn vector::VectorProgressSink>,
    mut delivery: Delivery<'_>,
) -> Result<String, kernel::error::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    let args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
    let audit = match crate::audit::Invocation::cli(&args) {
        Ok(audit) => audit,
        Err(error) => {
            let result = Err(error);
            if let Some(deliver) = delivery {
                let _ = deliver(&result);
            }
            return result;
        }
    };
    let _capture = crate::audit::result::Capture::begin();
    let result = command::cli::Cli::try_parse_from(args)
        .map_err(kernel::error::Error::Cli)
        .and_then(|cli| execute(cli, progress));
    if let Some(mut audit) = audit {
        let output = rendered(&result).0;
        let class = result.as_ref().err().and_then(|error| match error {
            kernel::error::Error::Cli(error) if !error.use_stderr() => None,
            _ => Some(crate::audit::error_class(error)),
        });
        let id = audit.id();
        let _ = audit.prepare(output.as_bytes(), class);
        let sent = if let Some(deliver) = delivery.as_mut() {
            deliver(&result)
        } else {
            Ok(())
        };
        if audit.settle(sent.is_err()).is_err() {
            eprintln!(
                "equill: audit pending for invocation {id}; the operation result is preserved"
            );
        }
        sent?;
    } else if let Some(deliver) = delivery {
        deliver(&result)?;
    }
    result
}

fn execute(
    cli: command::cli::Cli,
    progress: Option<&mut dyn vector::VectorProgressSink>,
) -> Result<String, kernel::error::Error> {
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
