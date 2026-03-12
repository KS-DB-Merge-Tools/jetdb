# フォーム/レポート デザイン定義の内部構造

Access データベース (.mdb/.accdb) におけるフォーム/レポートのデザイン定義の格納構造に関する調査知見。

## 概要

フォーム/レポートのデザイン定義は `MSysAccessStorage` システムテーブル（Jet4/ACE 形式）に格納される。バイナリフォーマットの公開仕様は存在せず、バイナリレベルで直接読み取りに成功している OSS も存在しない。

## MSysAccessStorage 内の格納構造

MSysAccessStorage は仮想ファイルシステム的なツリー構造を持つテーブルで、各行が1つのエントリ（フォルダまたはストリーム）を表す。

カラム構成:
- `Id` (Long, AUTO) - エントリID
- `ParentId` (Long) - 親エントリのID
- `Name` (Text) - エントリ名
- `Type` (Long) - 1=ストレージ（フォルダ）、2=ストリーム（データ）
- `Lv` (Ole) - バイナリデータ本体

### ツリー構造

```
Root (Id=1)
├── Forms (Type=1)
│   ├── DirData (Type=2)          フォーム名→ストレージ番号の対応表
│   ├── PropData (Type=2)         Formsフォルダのプロパティ
│   ├── DirDataCopy (Type=2)      DirData のバックアップ
│   ├── PropDataCopy (Type=2)     PropData のバックアップ
│   ├── "0" (Type=1)              個別フォーム（ストレージ番号 "0"）
│   │   ├── Blob (Type=2)           デザイン本体バイナリ (60-124KB)
│   │   ├── TypeInfo (Type=2)       コントロール名・型情報 (~1.6KB)
│   │   ├── PropData (Type=2)       プロパティ (22 bytes)
│   │   └── BlobDelta (Type=2)      差分データ（通常空）
│   ├── "1" (Type=1)              別のフォーム
│   │   └── (同じ4ストリーム)
│   ...
├── Reports (Type=1)              レポート（Forms と同一構造）
│   ├── DirData, PropData, ...
│   ├── "1" (Type=1)
│   │   └── Blob, TypeInfo, PropData, BlobDelta
│   ...
├── Modules (Type=1)              VBAモジュール
├── VBA (Type=1)                  VBAプロジェクト
│   └── VBAProject (Type=1)
│       ├── VBA/ (ストリーム群)
│       ├── PROJECT (Type=2)
│       └── PROJECTwm (Type=2)
├── Scripts, Cmdbars, Databases, CustomGroups, ImExSpecs
└── PropData (Type=2)             ルートプロパティ
```

## DirData フォーマット

DirData はフォーム表示名と内部ストレージ番号（"0", "1", ... 等の文字列）の対応表。

```
先頭4バイト: パディング（0x00）
繰り返し:
  0x04                  マーカーバイト
  <1バイト>             ペイロード長（バイト数）
  <UTF-16LE 文字列>     フォーム表示名 + ストレージ番号
終端4バイト: 0x00
```

ストレージ番号はペイロードの末尾 2 バイト（UTF-16LE の 1 文字）として格納される。

例:
- ストレージ "0" → F_クライアント情報
- ストレージ "1" → F_クライアント担当一覧
- ストレージ "5" → F_メニュー

## TypeInfo フォーマット

TypeInfo はフォーム内の全コントロールの名前と型コードの一覧。

```
ヘッダ:
  4バイト: マジックナンバー 0xACCDEAF7
  16バイト: GUID
  4バイト: エントリ数（推定）

各エントリ:
  コントロール名: Shift-JIS (cp932), NUL 終端
  型コード: 2バイト (Little-Endian)
  インデックス: 4バイト (Little-Endian)
```

含まれるコントロール例:
- ボタン: `Btn_クライアント一覧`
- テキストボックス: `Txt_ログイン中のユーザー名`
- チェックボックス: `Check_管理者`
- セクション: `フォームヘッダー`, `フォームフッター`, `詳細`

## Blob（デザイン本体）

フォームデザインの本体バイナリ。ControlSource、イベントプロシージャ、RecordSource 等の構造情報を含む。

- サイズ: 60KB - 124KB（フォームの複雑さに依存）
- 先頭バイト: `15 00 14 00 00 00 00 00 96 00 06 00 ...`（全フォーム共通のヘッダ）
- バイナリフォーマットは**未解明**。公開仕様なし

## SaveAsText との関係

`Application.SaveAsText` は Access COM 経由でフォーム定義をテキスト形式にエクスポートする機能。出力されるテキストには以下が含まれる:

- `Begin Form` ... `End` のネスト構造
- `ControlSource`, `RecordSource`, `Filter` 等のプロパティ
- `OnClick ="[Event Procedure]"` 等のイベントバインディング
- `CodeBehindForm` 以降に VBA ソースコード

SaveAsText の出力は Blob バイナリを Access が内部的にデシリアライズした結果であり、Blob のバイナリ構造を理解する上での参考になる。ただし SaveAsText の実行には Access ランタイムが必要なため、jetdb（ファイル直接読み取り）では使用できない。

## 先行事例

フォーム/レポートのバイナリ定義を直接読み取りに成功している OSS は存在しない。

| プロジェクト | 言語 | フォーム対応 |
|---|---|---|
| Jackcess | Java | 未対応。テーブル/クエリのみ |
| MDB Tools | C | カタログで名前認識のみ |
| access_parser | Python | Type=1(テーブル)のみ |
| msaccess-vcs-addin | VBA | SaveAsText 経由（Access 必要） |

MS-OFORMS 仕様は VBA UserForm 用であり、Access ネイティブフォームには適用不可。

## 参考リンク

- MDB Tools HACKING.md: https://github.com/mdbtools/mdbtools/blob/main/HACKING.md
- jabakobob.net Unofficial MDB Guide: http://jabakobob.net/mdb/
- isladogs.co.uk Long Value Binary fields: https://www.isladogs.co.uk/lv-fields/index.html
