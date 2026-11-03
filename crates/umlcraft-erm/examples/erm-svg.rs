//! Writes the SVG image of an ER diagram: `cargo run -p umlcraft-erm --example erm-svg --
//! FILE.ucerm [OUT.svg]` (without OUT to standard output).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(input) = args.first() else {
        eprintln!("usage: erm-svg FILE.ucerm [OUT.svg]");
        return ExitCode::FAILURE;
    };
    let text = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let diagram = match umlcraft_erm::read(&text) {
        Ok((d, warnings)) => {
            for w in warnings {
                eprintln!("{input}: warning: {w}");
            }
            d
        }
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let svg = umlcraft_erm::svg::render(&diagram);
    match args.get(1) {
        Some(out) => {
            if let Err(e) = std::fs::write(out, svg) {
                eprintln!("{out}: {e}");
                return ExitCode::FAILURE;
            }
        }
        None => print!("{svg}"),
    }
    ExitCode::SUCCESS
}
