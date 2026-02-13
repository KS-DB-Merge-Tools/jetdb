use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use jetdb::format::JetVersion;
use jetdb::PageReader;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Read-only tool for Microsoft Access (.mdb / .accdb) databases
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show database engine version
    Ver(VerArgs),
}

#[derive(Args)]
struct VerArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Show detailed version info
    #[arg(short, long)]
    long: bool,
}

// ---------------------------------------------------------------------------
// ver subcommand
// ---------------------------------------------------------------------------

fn cmd_ver(args: VerArgs) -> ExitCode {
    match run_ver(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jetdb: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_ver(args: &VerArgs) -> Result<(), jetdb::FileError> {
    let reader = PageReader::open(&args.file)?;
    let version = reader.header().version;

    if args.long {
        println!("{version}");
    } else {
        println!("{}", version_short_name(version));
    }
    Ok(())
}

fn version_short_name(v: JetVersion) -> &'static str {
    match v {
        JetVersion::Jet3 => "JET3",
        JetVersion::Jet4 => "JET4",
        JetVersion::Ace12 => "ACE12",
        JetVersion::Ace14 => "ACE14",
        JetVersion::Ace15 => "ACE15",
        JetVersion::Ace16 => "ACE16",
        JetVersion::Ace17 => "ACE17",
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Ver(args) => cmd_ver(args),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_all_versions() {
        assert_eq!(version_short_name(JetVersion::Jet3), "JET3");
        assert_eq!(version_short_name(JetVersion::Jet4), "JET4");
        assert_eq!(version_short_name(JetVersion::Ace12), "ACE12");
        assert_eq!(version_short_name(JetVersion::Ace14), "ACE14");
        assert_eq!(version_short_name(JetVersion::Ace15), "ACE15");
        assert_eq!(version_short_name(JetVersion::Ace16), "ACE16");
        assert_eq!(version_short_name(JetVersion::Ace17), "ACE17");
    }
}
