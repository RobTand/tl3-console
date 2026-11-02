use std::{io::Write, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tl3-console: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> tl3_console::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage =
        tl3_console::Error::Invalid("usage: tl3-console --config FILE [--public-host NAME]...");
    let mut path = None;
    let mut public_hosts = Vec::new();
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--config", Some(value)) if path.is_none() => path = Some(PathBuf::from(value)),
            // The tunnel's public name (#338), or a LAN name besides the listen address.
            ("--public-host", Some(value)) => public_hosts.push(value),
            _ => return Err(usage),
        }
    }
    let path = path.ok_or(usage)?;
    let config = tl3_config::Config::load_console(path)?;
    config.validate()?;
    let console = config.console.ok_or(tl3_console::Error::Invalid(
        "the config has no [console] section",
    ))?;
    let listener = std::net::TcpListener::bind(console.listen)?;
    let running = tl3_console::start(
        listener,
        tl3_console::Options {
            store: console.store(&config.store),
            assets: console.assets,
            models: Some(config.models),
            public_hosts,
        },
    )?;
    println!("Console listening on http://{}", running.address());
    std::io::stdout().flush()?;
    running.wait()
}
