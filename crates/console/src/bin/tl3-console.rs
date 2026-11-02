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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, traces) = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--config", path] => (PathBuf::from(path), None),
        ["--config", path, "--traces", traces] => (PathBuf::from(path), Some(traces)),
        _ => {
            return Err(tl3_console::Error::Invalid(
                "usage: tl3-console --config FILE [--traces MLFLOW_TRACES_URL]",
            ));
        }
    };
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
            links: tl3_console::case_file::Links::new(traces, &config.github.repos)?,
            models: Some(config.models),
        },
    )?;
    println!("Console listening on http://{}", running.address());
    std::io::stdout().flush()?;
    running.wait()
}
