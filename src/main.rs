fn main() {
    if let Err(err) = tempotui::run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}
