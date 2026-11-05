mod diagrams;
mod json;
mod logo_fit;
mod scenes;
mod sys;

use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--refresh-cache") {
        let _ = diagrams::refresh_cache_sync();
        return ExitCode::SUCCESS;
    }

    let bin_name = args
        .first()
        .and_then(|s| Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("fastfetch-scripts")
        .trim_end_matches(".py");

    let res = match bin_name {
        "diagrams" => diagrams::run(&args[1..]),
        "scenes" => scenes::run(&args[1..]),
        "logo-fit" => logo_fit::run(&args[1..]),
        _ => match args.get(1).map(|s| s.trim_end_matches(".py")) {
            Some("diagrams") => diagrams::run(&args[2..]),
            Some("scenes") => scenes::run(&args[2..]),
            Some("logo-fit") => logo_fit::run(&args[2..]),
            // Also allow direct scene names without subcommand!
            Some("potion" | "dungeon" | "orrery" | "grove" | "constellation") => {
                scenes::run(&args[1..])
            }
            Some("circuit" | "metro" | "strata" | "radar" | "fabric" | "scroll") => {
                diagrams::run(&args[1..])
            }
            _ => Err(String::from(
                "usage: fastfetch-scripts <diagrams|scenes|logo-fit> [args...]",
            )),
        },
    };

    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}

