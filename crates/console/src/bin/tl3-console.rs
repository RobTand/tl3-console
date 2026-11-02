use std::{io::Write, path::PathBuf, process::ExitCode};
use tl3_console::config::Settings;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tl3-console: {error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: tl3-console [check] --config FILE";

fn run() -> tl3_console::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (check, path) = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--config", path] => (false, PathBuf::from(path)),
        ["check", "--config", path] => (true, PathBuf::from(path)),
        _ => return Err(tl3_console::Error::Invalid(USAGE)),
    };
    let settings = Settings::load(path)?;
    if check {
        // Deploy runs this with the new binary before it replaces the old one.
        tl3_console::check(&settings.store)?;
        println!("Console config loads and the store's schema matches the pinned TL3");
        return Ok(());
    }
    let listener = std::net::TcpListener::bind(settings.listen)?;
    let running = tl3_console::start(
        listener,
        tl3_console::Options {
            store: settings.store,
            assets: settings.assets,
            models: Some(settings.models),
            links: settings.links,
            public_hosts: settings.public_hosts,
        },
    )?;
    println!("Console listening on http://{}", running.address());
    std::io::stdout().flush()?;
    running.wait()
}
