fn main() {
    match equill::run(std::env::args_os()) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("equill: {error}");
            std::process::exit(1);
        }
    }
}
