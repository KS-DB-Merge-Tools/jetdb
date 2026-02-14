# jetdb CLI

A read-only command-line tool for Microsoft Access database files (.mdb / .accdb).

## Installation

```bash
cargo install --path crates/jetdb-cli
```

## Subcommands

### ver — Show database engine version

```
jetdb ver [OPTIONS] <FILE>
```

Display the Jet/ACE engine version of the database file.

#### Options

- `-l`, `--long` — Show detailed version information

#### Output examples

```
$ jetdb ver testV2003.mdb
JET4

$ jetdb ver -l testV2003.mdb
Jet4 (Access 2000/2003)
```

#### Version list

Short name   Details
JET3         Jet3 (Access 97)
JET4         Jet4 (Access 2000/2003)
ACE12        ACE12 (Access 2007)
ACE14        ACE14 (Access 2010)
ACE15        ACE15 (Access 2013)
ACE16        ACE16 (Access 2016)
ACE17        ACE17 (Access 2019)

### tables — List tables

```
jetdb tables [OPTIONS] <FILE>
```

Output the table names in the database, one per line.

#### Options

- `-s`, `--system` — Include system tables (those with SYSTEM/HIDDEN flags)
- `-t`, `--show-type` — Prefix each line with the type number (tab-separated)
- `-T`, `--show-type-name` — Prefix each line with the type name (tab-separated)

`-t` and `-T` are mutually exclusive (cannot be used together).

#### Output examples

```
$ jetdb tables test.mdb
Table1
Table2

$ jetdb tables -s test.mdb
MSysObjects
MSysACEs
Table1
Table2

$ jetdb tables -t test.mdb
1	Table1
1	Table2

$ jetdb tables -T test.mdb
table	Table1
table	Table2

$ jetdb tables -s -T test.mdb
systable	MSysObjects
systable	MSysACEs
table	Table1
table	Table2
```

#### Type names

Type names displayed with -T:

Name          Condition
table         Regular user table
systable      Table with SYSTEM or HIDDEN flag

### schema — Show table schema

```
jetdb schema [OPTIONS] <FILE>
```

Display column definitions, indexes, and relationships of tables.
When the `--ddl` option is specified, output as SQL DDL (CREATE TABLE, etc.).

#### Options

- `-T`, `--table <NAME>` — Show only the specified table
- `--ddl <DIALECT>` — Output as DDL in the specified dialect (sqlite, postgres, mysql, access)
- `--no-indexes` — Omit index definitions
- `--no-relations` — Omit relationship definitions

#### Output examples

```
$ jetdb schema test.mdb -T Table1
Table: Table1

  Columns:
    A  Text(100)
    B  Text(200)
    C  Byte
    D  Int
    E  Long
    F  Double
    G  Timestamp
    H  Money
    I  Boolean

  Indexes:
    B           [B ASC]
    PrimaryKey  [A ASC]  UNIQUE REQUIRED

$ jetdb schema test.mdb --ddl sqlite -T Table1
CREATE TABLE "Table1" (
    "A" TEXT,
    "B" TEXT,
    "C" INTEGER,
    "D" INTEGER,
    "E" INTEGER,
    "F" REAL,
    "G" TEXT,
    "H" NUMERIC,
    "I" INTEGER,
    PRIMARY KEY ("A")
);

CREATE INDEX "B" ON "Table1" ("B");

$ jetdb schema indexTest.mdb --ddl postgres
CREATE TABLE "Table1" (
    "id" INTEGER,
    "otherfk1" INTEGER,
    "otherfk2" INTEGER,
    "data" VARCHAR(100),
    "otherfk3" INTEGER,
    PRIMARY KEY ("id")
);

CREATE TABLE "Table2" (
    "id" INTEGER,
    "data" VARCHAR(100),
    PRIMARY KEY ("id")
);

CREATE TABLE "Table3" (
    "id" INTEGER,
    "data" VARCHAR(100),
    PRIMARY KEY ("id")
);

CREATE INDEX "id" ON "Table1" ("id");

CREATE INDEX "id" ON "Table2" ("id");

CREATE INDEX "id" ON "Table3" ("id");

ALTER TABLE "Table1" ADD CONSTRAINT "Table2Table1"
    FOREIGN KEY ("otherfk1") REFERENCES "Table2" ("id")
    ON DELETE CASCADE;
ALTER TABLE "Table1" ADD CONSTRAINT "Table3Table1"
    FOREIGN KEY ("otherfk2") REFERENCES "Table3" ("id")
    ON UPDATE CASCADE;
```

#### DDL dialect list

Name       Description
sqlite     SQLite
postgres   PostgreSQL
mysql      MySQL
access     Access SQL

### queries — Manage saved queries

#### queries list — List saved query names

```
jetdb queries list [OPTIONS] <FILE>
```

List the names of saved queries in the database (space-separated by default).

##### Options

- `-1`, `--newline` — Print one query name per line
- `-d`, `--delimiter <STRING>` — Custom delimiter between query names (default: space)

##### Output examples

```
$ jetdb queries list queryTest.mdb
SelectQuery UnionQuery CrosstabQuery DeleteQuery UpdateQuery AppendQuery MakeTableQuery PassthroughQuery DataDefinitionQuery

$ jetdb queries list -1 queryTest.mdb
SelectQuery
UnionQuery
CrosstabQuery
DeleteQuery
UpdateQuery
AppendQuery
MakeTableQuery
PassthroughQuery
DataDefinitionQuery

$ jetdb queries list test.mdb
(no output — database has no saved queries)
```

#### queries show — Show query SQL

```
jetdb queries show <FILE> <QUERY_NAME>
```

Show the restored SQL definition of the specified saved query.

##### Output examples

```
$ jetdb queries show queryTest.mdb DeleteQuery
DELETE [Table1].[col1], [Table1].[col2], [Table1].[col3]
FROM [Table1]
WHERE (([Table1].[col1]="foo"));
```

### export — Export table data as CSV

```
jetdb export [OPTIONS] <FILE> <TABLE>
```

Export all rows from a table as RFC 4180 compliant CSV to standard output.
By default, replication system columns are excluded.

#### Options

- `-H`, `--no-header` — Suppress the header row
- `-d`, `--delimiter <CHAR>` — Column delimiter (default: `,`)
- `-D`, `--date-format <FMT>` — Date format, strftime subset (default: `%Y-%m-%d`)
- `-T`, `--datetime-format <FMT>` — Date-time format, strftime subset (default: `%Y-%m-%d %H:%M:%S`)
- `-b`, `--bin <MODE>` — Binary output mode (default: `hex`)
- `-0`, `--null <STRING>` — String to represent NULL values (default: empty string)
- `-B`, `--boolean-words` — Output booleans as TRUE/FALSE instead of 1/0
- `-s`, `--system-columns` — Include replication system columns

#### Output examples

```
$ jetdb export test.mdb Table1
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -H
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -d "\t"
A	B	C	D	E	F	G	H	I

$ jetdb export test.mdb Table1 -B
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",TRUE
```

#### Binary output modes

Name     Description
strip    Omit binary data (empty string)
raw      Output as raw bytes (lossy UTF-8)
octal    Each byte as \NNN octal escape
hex      Each byte as lowercase hex (default)
