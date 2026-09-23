#![forbid(unsafe_code)]
use clap::Parser;
use skill_bom::{application, domain::Error, env, interfaces, process};
fn main() -> std::process::ExitCode {
    let args = env::arguments();
    let json = args
        .iter()
        .any(|a| a == "--format=json" || a == "--format=spdx-json")
        || args
            .windows(2)
            .any(|a| a[0] == "--format" && (a[1] == "json" || a[1] == "spdx-json"));
    let cli = match interfaces::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if error.exit_code() == 0 => {
            let _ = error.print();
            return 0.into();
        }
        Err(error) => {
            interfaces::error(&Error::new("INPUT", error.to_string(), 2), json);
            return 2.into();
        }
    };
    let result = process::install_interrupt_handler()
        .and_then(|()| application::run(&cli))
        .and_then(|out| interfaces::render(out, cli.format));
    match result {
        Ok(code) => code.into(),
        Err(e) => {
            interfaces::error(&e, cli.format != interfaces::Format::Text);
            e.exit_code.into()
        }
    }
}
