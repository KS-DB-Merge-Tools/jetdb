---
name: jetdb-cli
description: >
  Read-only CLI for Microsoft Access databases (.mdb/.accdb).
  List tables, show schemas, export rows as CSV, generate DDL
  (SQLite/PostgreSQL/MySQL/Access), extract VBA source code,
  show saved query SQL, and view object properties.
  Supports Jet3 (Access 97) through ACE17 (Access 2019),
  including password-protected and encrypted .accdb files.
  Use when migrating Access to SQL, extracting data from .mdb files,
  or inspecting legacy database structure.
---

# jetdb CLI

Read-only CLI for Microsoft Access (.mdb/.accdb) databases. Single binary, no runtime dependencies.

```
cargo install jetdb-cli
```

## Quick Reference

```
jetdb [--password <PASS>] <COMMAND> [OPTIONS] <FILE> [ARGS]
```

All output goes to stdout. Errors go to stderr prefixed with `jetdb:`.
Exit 0 on success, 1 on error. List commands produce empty output (exit 0) when no data exists.
Access object names often contain spaces — always quote table, query, and module name arguments (e.g., `jetdb export data.mdb "Order Details"`).

## Commands

### ver — Engine version

```
jetdb ver <FILE>              # Short: JET3, JET4, ACE12..ACE17
jetdb ver -l <FILE>           # Long: "Jet4 (Access 2000/2003)"
```

### tables — List tables

```
jetdb tables <FILE>           # User tables, one per line, sorted
jetdb tables -s <FILE>        # Include system tables
jetdb tables -T <FILE>        # Prefix type name (table/systable), tab-separated
jetdb tables -t <FILE>        # Prefix type number, tab-separated
```

`-t` and `-T` are mutually exclusive.

Output example:

```
$ jetdb tables -s -T data.mdb
systable	MSysACEs
systable	MSysObjects
table	Customers
table	Orders
```

### schema — Table structure and DDL

```
jetdb schema <FILE>                            # All tables, human-readable
jetdb schema <FILE> -T <TABLE>                 # Single table
jetdb schema <FILE> --ddl sqlite               # DDL: sqlite|postgres|mysql|access
jetdb schema <FILE> --no-indexes --no-relations
```

Human-readable and DDL output both show Columns, Indexes, and Relationships.
`--no-indexes` and `--no-relations` apply to both output modes.
DDL generates CREATE TABLE, CREATE INDEX, and ALTER TABLE FOREIGN KEY.

Output example (human-readable):

```
Table: Customers

  Columns:
    ID    Long         NOT NULL AUTO
    Name  Text(100)
    Email Text(200)

  Indexes:
    PrimaryKey  [ID ASC]  UNIQUE REQUIRED
```

### export — CSV export

```
jetdb export <FILE> <TABLE>                    # RFC 4180 CSV to stdout
jetdb export <FILE> <TABLE> -H                 # No header row
jetdb export <FILE> <TABLE> -d "\t"            # Tab-delimited
jetdb export <FILE> <TABLE> -D "%d/%m/%Y"      # Custom date format (strftime)
jetdb export <FILE> <TABLE> -T "%Y-%m-%dT%H:%M:%S"  # Custom datetime
jetdb export <FILE> <TABLE> -b strip           # Binary: strip|raw|octal|hex
jetdb export <FILE> <TABLE> -0 "NULL"          # Custom NULL string
jetdb export <FILE> <TABLE> -B                 # Booleans as TRUE/FALSE
jetdb export <FILE> <TABLE> -s                 # Include replication columns
```

Text and GUID values are always quoted. Numeric values are unquoted.

Output example:

```
ID,Name,Email,Active
1,"Alice","alice@example.com",1
2,"Bob","bob@example.com",0
```

### queries — Saved queries

```
jetdb queries list <FILE>                      # Space-separated, sorted
jetdb queries list -1 <FILE>                   # One per line
jetdb queries show <FILE> <QUERY_NAME>         # Print SQL definition
```

### vba — VBA modules

```
jetdb vba list <FILE>                          # Space-separated, sorted
jetdb vba list -1 <FILE>                       # One per line
jetdb vba show <FILE> <MODULE_NAME>            # Print full source code
```

### prop — Object properties

```
jetdb prop <FILE> <OBJECT_NAME>
```

Shows LvProp values grouped by: Table Properties, Column (per-column), Additional Properties.

## Encrypted Databases

.mdb files (Access 97-2003) use Jet RC4 encoding handled transparently — no password needed.
.accdb files (Access 2007+) may require a password:

```
jetdb --password "secret" tables protected.accdb
```

`--password` is global and must appear before the subcommand.

## Workflow Patterns

### Explore a database

```
jetdb ver data.mdb
jetdb tables data.mdb
jetdb schema data.mdb
```

### Export for analysis

```
jetdb tables data.mdb                          # Find table names
jetdb schema data.mdb -T Customers             # Check columns
jetdb export data.mdb Customers > customers.csv
```

### Migrate schema to another RDBMS

```
jetdb schema data.mdb --ddl postgres > schema.sql
```

### Extract all VBA modules

```
jetdb vba list -1 data.mdb | while read -r mod; do
  jetdb vba show data.mdb "$mod" > "${mod}.bas"
done
```

### Review all saved queries

```
jetdb queries list -1 data.mdb | while read -r q; do
  echo "=== $q ==="
  jetdb queries show data.mdb "$q"
done
```

## Error Behavior

- File not found / unreadable → exit 1
- Table/query/module not found on `show` commands → exit 1
- Password required but missing → exit 1, `PasswordRequired`
- Wrong password → exit 1, `InvalidPassword`
- `list` with no data → exit 0, empty stdout
