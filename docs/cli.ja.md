# jetdb CLI

Microsoft Access データベース (.mdb / .accdb) の読み取り専用コマンドラインツール。

## インストール

```bash
cargo install --path crates/jetdb-cli
```

## サブコマンド

### ver — データベースエンジンバージョンの表示

```
jetdb ver [OPTIONS] <FILE>
```

データベースファイルの Jet/ACE エンジンバージョンを表示する。

#### オプション

- `-l`, `--long` — 詳細なバージョン情報を表示

#### 出力例

```
$ jetdb ver testV2003.mdb
JET4

$ jetdb ver -l testV2003.mdb
Jet4 (Access 2000/2003)
```

#### バージョン一覧

短縮名   詳細
JET3    Jet3 (Access 97)
JET4    Jet4 (Access 2000/2003)
ACE12   ACE12 (Access 2007)
ACE14   ACE14 (Access 2010)
ACE15   ACE15 (Access 2013)
ACE16   ACE16 (Access 2016)
ACE17   ACE17 (Access 2019)

### tables — テーブル一覧の表示

```
jetdb tables [OPTIONS] <FILE>
```

データベースに含まれるテーブル名を1行1テーブルで出力する。

#### オプション

- `-s`, `--system` — システムテーブル (SYSTEM/HIDDEN フラグ付き) を含める
- `-t`, `--show-type` — 種別番号をタブ区切りで前置表示
- `-T`, `--show-type-name` — 種別名をタブ区切りで前置表示

`-t` と `-T` は排他 (同時指定不可)。

#### 出力例

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

#### 種別名

-T で表示される種別名:

名前          条件
table        通常のユーザーテーブル
systable     SYSTEM または HIDDEN フラグ付きテーブル

### schema — テーブルスキーマの表示

```
jetdb schema [OPTIONS] <FILE>
```

テーブルのカラム定義・インデックス・リレーションシップを表示する。
`--ddl` オプション指定時は SQL の DDL (CREATE TABLE 等) として出力する。

#### オプション

- `-T`, `--table <NAME>` — 指定テーブルのみ表示
- `--ddl <DIALECT>` — 指定方言の DDL として出力 (sqlite, postgres, mysql, access)
- `--no-indexes` — インデックス定義を省略
- `--no-relations` — リレーションシップ定義を省略

#### 出力例

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

#### DDL 方言一覧

名前       説明
sqlite     SQLite
postgres   PostgreSQL
mysql      MySQL
access     Access SQL
