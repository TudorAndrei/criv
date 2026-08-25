fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match criv::run(&args) {
        Ok(()) => 0,
        Err(err) => {
            if !err.is_reported() {
                eprintln!("criv: {err}");
            }
            err.exit_code()
        }
    };

    std::process::exit(code);
}
