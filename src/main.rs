fn main() {
    match equill::run_cli(std::env::args_os()) {
        // A command that speaks its own protocol on stdout — the MCP adapter —
        // returns nothing to print. Adding a newline there would inject a stray
        // frame into a stream a client is parsing.
        Ok(output) if output.is_empty() => {}
        Ok(output) => println!("{output}"),
        Err(error) if error.command_output().is_some() => {
            println!("{}", error.command_output().expect("checked output"));
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("equill: {error}");
            std::process::exit(1);
        }
    }
}
