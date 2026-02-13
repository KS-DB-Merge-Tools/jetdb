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
