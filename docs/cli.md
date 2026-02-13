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
