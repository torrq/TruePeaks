use truepeaks::cli::{execute_headless, parse_cli_args_headless, CliAction};
use truepeaks::config::AppConfig;

fn main() {
    let cfg = AppConfig::load();
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse_cli_args_headless(&args, &cfg) {
        CliAction::LaunchGui(_) => {
            std::process::exit(0);
        }
        CliAction::PrintAndExit(code, msg) => {
            if code == 0 {
                println!("{}", msg);
            } else {
                eprintln!("{}", msg);
            }
            std::process::exit(code);
        }
        CliAction::RunHeadless {
            target,
            config,
            format,
            output_path,
            show_progress,
        } => match execute_headless(target, config, format, output_path.as_deref(), show_progress) {
            Ok(text) => {
                if output_path.is_none() {
                    println!("{}", text);
                }
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        },
    }
}
