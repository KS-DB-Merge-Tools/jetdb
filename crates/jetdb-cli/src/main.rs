use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use jetdb::format::{catalog_flags, JetVersion, ObjectType};
use jetdb::{read_catalog, CatalogEntry, PageReader};

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
    /// List table names
    Tables(TablesArgs),
}

#[derive(Args)]
struct VerArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Show detailed version info
    #[arg(short, long)]
    long: bool,
}

#[derive(Args)]
struct TablesArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Include system tables
    #[arg(short = 's', long = "system")]
    system: bool,

    /// Show object type number
    #[arg(short = 't', long = "show-type", conflicts_with = "show_type_name")]
    show_type: bool,

    /// Show object type name
    #[arg(short = 'T', long = "show-type-name", conflicts_with = "show_type")]
    show_type_name: bool,
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
// tables subcommand
// ---------------------------------------------------------------------------

fn cmd_tables(args: TablesArgs) -> ExitCode {
    match run_tables(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jetdb: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_tables(args: &TablesArgs) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open(&args.file)?;
    let catalog = read_catalog(&mut reader)?;

    for entry in &catalog {
        if !should_show(entry, args.system) {
            continue;
        }
        if args.show_type {
            println!("{}\t{}", entry.object_type as i32, entry.name);
        } else if args.show_type_name {
            println!("{}\t{}", resolve_type_name(entry), entry.name);
        } else {
            println!("{}", entry.name);
        }
    }
    Ok(())
}

fn should_show(entry: &CatalogEntry, include_system: bool) -> bool {
    if entry.object_type != ObjectType::Table {
        return false;
    }
    if include_system {
        return true;
    }
    (entry.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN)) == 0
}

fn resolve_type_name(entry: &CatalogEntry) -> &'static str {
    if entry.object_type == ObjectType::Table
        && (entry.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN)) != 0
    {
        "systable"
    } else {
        object_type_name(entry.object_type)
    }
}

fn object_type_name(t: ObjectType) -> &'static str {
    match t {
        ObjectType::Form => "form",
        ObjectType::Table => "table",
        ObjectType::Macro => "macro",
        ObjectType::SystemTable => "systemtable",
        ObjectType::Report => "report",
        ObjectType::Query => "query",
        ObjectType::LinkedTable => "linkedtable",
        ObjectType::Module => "module",
        ObjectType::Relationship => "relationship",
        ObjectType::DatabaseProperty => "dbproperty",
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Ver(args) => cmd_ver(args),
        Commands::Tables(args) => cmd_tables(args),
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

    // -- tables helpers -------------------------------------------------------

    fn entry(name: &str, object_type: ObjectType, flags: u32) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            object_type,
            table_page: 100,
            flags,
        }
    }

    // should_show tests

    #[test]
    fn should_show_user_table() {
        let e = entry("Users", ObjectType::Table, 0);
        assert!(should_show(&e, false));
    }

    #[test]
    fn should_show_excludes_system_by_default() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert!(!should_show(&e, false));
    }

    #[test]
    fn should_show_includes_system_with_flag() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert!(should_show(&e, true));
    }

    #[test]
    fn should_show_excludes_hidden_by_default() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert!(!should_show(&e, false));
    }

    #[test]
    fn should_show_includes_hidden_with_flag() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert!(should_show(&e, true));
    }

    #[test]
    fn should_show_excludes_non_table() {
        let e = entry("MyQuery", ObjectType::Query, 0);
        assert!(!should_show(&e, false));
        assert!(!should_show(&e, true));
    }

    // resolve_type_name tests

    #[test]
    fn resolve_type_name_normal_table() {
        let e = entry("Users", ObjectType::Table, 0);
        assert_eq!(resolve_type_name(&e), "table");
    }

    #[test]
    fn resolve_type_name_system_table() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert_eq!(resolve_type_name(&e), "systable");
    }

    #[test]
    fn resolve_type_name_hidden_table() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert_eq!(resolve_type_name(&e), "systable");
    }

    // object_type_name tests

    #[test]
    fn object_type_name_all_variants() {
        assert_eq!(object_type_name(ObjectType::Form), "form");
        assert_eq!(object_type_name(ObjectType::Table), "table");
        assert_eq!(object_type_name(ObjectType::Macro), "macro");
        assert_eq!(object_type_name(ObjectType::SystemTable), "systemtable");
        assert_eq!(object_type_name(ObjectType::Report), "report");
        assert_eq!(object_type_name(ObjectType::Query), "query");
        assert_eq!(object_type_name(ObjectType::LinkedTable), "linkedtable");
        assert_eq!(object_type_name(ObjectType::Module), "module");
        assert_eq!(object_type_name(ObjectType::Relationship), "relationship");
        assert_eq!(object_type_name(ObjectType::DatabaseProperty), "dbproperty");
    }
}
