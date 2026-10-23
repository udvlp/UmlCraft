//! Exports a project, or the sample project, to an `.asta` file through the installed Astah:
//! `cargo run -p umlcraft-asta-export --example asta-export -- [in.ucraft] out.asta`.

use std::path::PathBuf;

use umlcraft_asta_export::Astah;

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let (model, out) = match &args[..] {
        [out] => (umlcraft_model::sample::order_system().0.model().clone(), out),
        [input, out] => (umlcraft_format::load(input).expect("load"), out),
        _ => panic!("usage: asta-export [in.ucraft] out.asta"),
    };
    let astah = Astah::find().expect("no Astah installation with the API found");
    println!("using {}", astah.home().display());
    let report = umlcraft_asta_export::export(&astah, &model, out).expect("export");
    println!("dropped: {:?}", report.dropped);
    for problem in &report.problems {
        println!("problem: {problem}");
    }
}
