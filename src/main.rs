use clap::Parser;

fn main() {
    let cli = graphia::cli::Cli::parse();
    if let Err(err) = graphia::cli::run(cli) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
