fn main() {
    if let Err(err) = tempotui::run() {
        eprintln!("tempotui: {err}");
        std::process::exit(1);
    }
}
