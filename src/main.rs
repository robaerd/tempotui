fn main() {
    if let Err(err) = tempo_log::run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}
