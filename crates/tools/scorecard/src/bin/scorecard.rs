use std::fs;
use std::path::PathBuf;

use aether_scorecard::{generate_scorecard, load_samples, render_csv, render_markdown};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "aether-scorecard")]
#[command(about = "Generate validator scorecards from metrics JSON")]
struct Args {
    /// Input JSON file containing an array of validator samples
    #[arg(long)]
    input: PathBuf,

    /// Output path for the markdown table. Prints to stdout if omitted.
    #[arg(long)]
    markdown_out: Option<PathBuf>,

    /// Output path for CSV summary. Skips writing if omitted.
    #[arg(long)]
    csv_out: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let payload = fs::read_to_string(&args.input)?;
    let samples = load_samples(&payload)?;
    let entries = generate_scorecard(&samples)?;

    let markdown = render_markdown(&entries);
    if let Some(path) = &args.markdown_out {
        fs::write(path, &markdown)?;
    } else {
        println!("{}", markdown);
    }

    if let Some(path) = &args.csv_out {
        let csv = render_csv(&entries);
        fs::write(path, csv)?;
    }

    Ok(())
}
