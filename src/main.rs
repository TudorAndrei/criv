fn main() {
    let code = match criv::run(std::env::args().skip(1).collect()) {
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
