use std::env;

fn main() {
    if let Err(error) = windows_pe_specialization_spike::parse_args(env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
