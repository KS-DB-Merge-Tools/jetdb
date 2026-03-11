---
name: jetdb-cli
description: >
  Microsoft Access データベース (.mdb/.accdb) の読み取り専用 CLI。
  テーブル一覧、スキーマ表示、CSV エクスポート、DDL 生成
  (SQLite/PostgreSQL/MySQL/Access)、VBA ソースコード抽出、
  保存済みクエリの SQL 表示、オブジェクトプロパティ表示が可能。
  Jet3 (Access 97) から ACE17 (Access 2019) まで対応。
  パスワード保護・暗号化された .accdb ファイルにも対応。
  Access から SQL への移行、.mdb ファイルからのデータ抽出、
  レガシーデータベース構造の調査に使用。
---

# jetdb CLI

Microsoft Access (.mdb/.accdb) データベースの読み取り専用 CLI。単一バイナリ、ランタイム依存なし。

```
cargo install jetdb-cli
```

## クイックリファレンス

```
jetdb [--password <PASS>] <COMMAND> [OPTIONS] <FILE> [ARGS]
```

全出力は stdout へ。エラーは stderr に `jetdb:` プレフィックス付きで出力。
終了コードは成功時 0、エラー時 1。一覧系コマンドはデータなしの場合、空出力で成功 (exit 0)。
Access のオブジェクト名にはスペースが含まれることが多い — テーブル名、クエリ名、モジュール名の引数は常にクォートすること（例: `jetdb export data.mdb "Order Details"`）。

## コマンド

### ver — エンジンバージョン

```
jetdb ver <FILE>              # 短縮形: JET3, JET4, ACE12..ACE17
jetdb ver -l <FILE>           # 詳細: "Jet4 (Access 2000/2003)"
```

### tables — テーブル一覧

```
jetdb tables <FILE>           # ユーザーテーブル、1行1件、名前ソート
jetdb tables -s <FILE>        # システムテーブルを含む
jetdb tables -T <FILE>        # 種別名 (table/systable) をタブ区切りで前置
jetdb tables -t <FILE>        # 種別番号をタブ区切りで前置
```

`-t` と `-T` は排他的。

出力例:

```
$ jetdb tables -s -T data.mdb
systable	MSysACEs
systable	MSysObjects
table	Customers
table	Orders
```

### schema — テーブル構造と DDL

```
jetdb schema <FILE>                            # 全テーブル、人間可読形式
jetdb schema <FILE> -T <TABLE>                 # 単一テーブル
jetdb schema <FILE> --ddl sqlite               # DDL: sqlite|postgres|mysql|access
jetdb schema <FILE> --no-indexes --no-relations
```

人間可読形式と DDL の両方でカラム、インデックス、リレーションシップを表示。
`--no-indexes` と `--no-relations` は両方の出力モードに適用される。
DDL は CREATE TABLE, CREATE INDEX, ALTER TABLE FOREIGN KEY を生成。

出力例 (人間可読形式):

```
Table: Customers

  Columns:
    ID    Long         NOT NULL AUTO
    Name  Text(100)
    Email Text(200)

  Indexes:
    PrimaryKey  [ID ASC]  UNIQUE REQUIRED
```

### export — CSV エクスポート

```
jetdb export <FILE> <TABLE>                    # RFC 4180 CSV を stdout へ
jetdb export <FILE> <TABLE> -H                 # ヘッダー行なし
jetdb export <FILE> <TABLE> -d "\t"            # タブ区切り
jetdb export <FILE> <TABLE> -D "%d/%m/%Y"      # 日付フォーマット (strftime)
jetdb export <FILE> <TABLE> -T "%Y-%m-%dT%H:%M:%S"  # 日時フォーマット
jetdb export <FILE> <TABLE> -b strip           # バイナリ: strip|raw|octal|hex
jetdb export <FILE> <TABLE> -0 "NULL"          # NULL の表現文字列
jetdb export <FILE> <TABLE> -B                 # 真偽値を TRUE/FALSE で出力
jetdb export <FILE> <TABLE> -s                 # レプリケーション列を含む
```

テキスト値と GUID は常に引用符付き。数値は引用符なし。

出力例:

```
ID,Name,Email,Active
1,"Alice","alice@example.com",1
2,"Bob","bob@example.com",0
```

### queries — 保存済みクエリ

```
jetdb queries list <FILE>                      # スペース区切り、ソート済み
jetdb queries list -1 <FILE>                   # 1行1件
jetdb queries show <FILE> <QUERY_NAME>         # SQL 定義を出力
```

### vba — VBA モジュール

```
jetdb vba list <FILE>                          # スペース区切り、ソート済み
jetdb vba list -1 <FILE>                       # 1行1件
jetdb vba show <FILE> <MODULE_NAME>            # ソースコード全体を出力
```

### prop — オブジェクトプロパティ

```
jetdb prop <FILE> <OBJECT_NAME>
```

LvProp 値をテーブルプロパティ、カラム別、追加プロパティにグループ化して表示。

## 暗号化データベース

.mdb ファイル (Access 97-2003) は Jet RC4 エンコーディングを透過的に処理 — パスワード不要。
.accdb ファイル (Access 2007+) はパスワードが必要な場合がある:

```
jetdb --password "secret" tables protected.accdb
```

`--password` はグローバルオプションで、サブコマンドの前に指定する。

## ワークフローパターン

### データベースの探索

```
jetdb ver data.mdb
jetdb tables data.mdb
jetdb schema data.mdb
```

### 分析用にエクスポート

```
jetdb tables data.mdb                          # テーブル名を確認
jetdb schema data.mdb -T Customers             # カラムを確認
jetdb export data.mdb Customers > customers.csv
```

### スキーマを他の RDBMS に移行

```
jetdb schema data.mdb --ddl postgres > schema.sql
```

### 全 VBA モジュールの抽出

```
jetdb vba list -1 data.mdb | while read -r mod; do
  jetdb vba show data.mdb "$mod" > "${mod}.bas"
done
```

### 全保存済みクエリの確認

```
jetdb queries list -1 data.mdb | while read -r q; do
  echo "=== $q ==="
  jetdb queries show data.mdb "$q"
done
```

## エラー動作

- ファイルが見つからない / 読み取り不可 → exit 1
- `show` コマンドでテーブル/クエリ/モジュールが見つからない → exit 1
- パスワードが必要だが未指定 → exit 1, `PasswordRequired`
- パスワードが不正 → exit 1, `InvalidPassword`
- `list` でデータなし → exit 0, stdout は空
