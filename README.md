# jetdb

[![CI](https://github.com/dominion525/jetdb/actions/workflows/ci.yml/badge.svg)](https://github.com/dominion525/jetdb/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![MSRV: 1.82](https://img.shields.io/badge/MSRV-1.82-orange)](https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html)

[日本語版 (Japanese)](README.ja.md)

A Rust library and CLI tool for reading Microsoft Access database files (.mdb / .accdb).

Written in pure Rust with no C/C++ dependencies or FFI, so it works out of the box on macOS, Windows, and Linux. Supports Access 97 (Jet3) through Access 2019 (ACE17).

## Installation

### CLI Tool

```bash
cargo install jetdb-cli
```

### As a Library

```toml
[dependencies]
jetdb = "0.1.1"
```

## CLI Usage

```bash
# Show the database engine version
jetdb ver database.mdb

# List tables
jetdb tables database.mdb

# Show table schema (columns, indexes, relationships)
jetdb schema database.mdb
jetdb schema database.mdb -T Table1

# Generate DDL (SQLite / PostgreSQL / MySQL / Access)
jetdb schema database.mdb --ddl sqlite

# Export table data as CSV
jetdb export database.mdb Table1

# List saved queries / show SQL
jetdb queries list database.mdb
jetdb queries show database.mdb SelectQuery

# List VBA modules / show source code
jetdb vba list database.mdb
jetdb vba show database.mdb Module1

# Show object properties
jetdb prop database.mdb Table1
```

See [docs/cli.md](docs/cli.md) for detailed options and output examples.

## Library Usage

```rust
use jetdb::{PageReader, read_catalog, read_table_def, read_table_rows};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;

    // List tables
    let catalog = read_catalog(&mut reader)?;
    for entry in &catalog {
        println!("{}", entry.name);
    }

    // Read table definition
    let entry = &catalog[0];
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // Read row data
    let result = read_table_rows(&mut reader, &table_def)?;
    for row in &result.rows {
        println!("{:?}", row);
    }

    Ok(())
}
```

For detailed API documentation and more examples, run `cargo doc --open` or see [docs.rs/jetdb](https://docs.rs/jetdb) (available after crates.io publication).

## Supported Versions

| Engine | Access Version | File Format |
|--------|---------------|-------------|
| Jet3   | Access 97      | .mdb       |
| Jet4   | Access 2000/2003 | .mdb     |
| ACE12  | Access 2007    | .accdb     |
| ACE14  | Access 2010    | .accdb     |
| ACE15  | Access 2013    | .accdb     |
| ACE16  | Access 2016    | .accdb     |
| ACE17  | Access 2019    | .accdb     |

## Features

- Read table metadata (columns, indexes, relationships)
- Read row data with Rust type mapping (Text, Long, Double, Timestamp, Money, Memo, OLE, GUID, etc.)
- Generate DDL for SQLite, PostgreSQL, MySQL, and Access SQL
- Recover SQL from saved queries
- Extract VBA module source code
- Read object properties (LvProp)
- Decrypt RC4-encrypted databases
- Handle Jet3 (Latin-1) and Jet4+ (UTF-16LE, compressed text) encodings

## Limitations

- Read-only (no write support)
- No index-based lookups (full table scan only)
- Loads all rows into memory; be mindful of memory usage with very large tables
- Password-protected databases are not supported
- Replication databases (.mda) are untested
- Multi-page overflow rows (LOOKUP_FLAG) are skipped; some large memo/OLE fields may be missing

## Acknowledgments

- [mdbtools](https://github.com/mdbtools/mdbtools) — [HACKING.md](https://github.com/mdbtools/mdbtools/blob/main/HACKING.md) was an invaluable reference for understanding the MDB/ACCDB file format
- [Jackcess](https://github.com/spannm/jackcess) (Apache License 2.0) — most test .mdb/.accdb files are sourced from this project (some were created independently)

## License

MIT OR Apache-2.0
