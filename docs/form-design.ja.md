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

- サイズ: 918B（シンプル）〜 52KB+（複雑なフォーム）
- 先頭バイト: `15 00 14 00 00 00 00 00`（全フォーム共通のヘッダ）
- バイナリフォーマットの公開仕様なし。以下はリバースエンジニアリングによる解析結果

### Blob 全体レイアウト

```
[ヘッダ 8B]
  version(u16) = 0x0015, flags(u16) = 0x0014, reserved(u32) = 0
[フォームレベル固定長プロパティ群]
  RecordSource, Filter, FontName 等のフォーム全体に関わるプロパティ
[フォームレベル可変長プロパティ群]
[バイナリデータ（レイアウト情報、プリンタ設定、フォント定義等）]
[コントロール別プロパティブロック ×N]
  各コントロールが独立したプロパティブロックを持つ
  Name(0x14) プロパティで始まる
```

### プロパティエントリのフォーマット

プロパティ名は文字列として格納されず、数値 ID (u16) で識別される。

**可変長プロパティ（文字列系）:**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + byte_length(u32) + UTF-16LE data
```

type の値:
- 0x0A — 名前・フォント系（Name, FontName, Format, RowSourceType, ColumnWidths）
- 0x0C — テキスト・データ系（RecordSource, ControlSource, RowSource, Filter, Event, Caption）

**固定長プロパティ（数値系）:**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + data[type依存]
```

type の値:
- 0x01 — Bool (4B)
- 0x02 — Short (4B)
- 0x03 — Long (6B)
- 0x04 — Color (8B)
- 0x08 — Double (8B)
- 0x09 — GUID (16B)
- 0x0B — Binary (可変長)

### 判明しているプロパティ ID マッピング

```
ID      プロパティ名      カテゴリ
------  ----------------  ------------------
0x0011  Caption           コントロール表示テキスト
0x0012  ColumnWidths      コンボボックス列幅
0x0014  Name              コントロール名
0x001B  ControlSource     フィールドバインド先
0x0022  FontName          フォント名
0x0026  Format            表示書式
0x005B  RowSource         コンボボックスデータソース (SQL)
0x005D  RowSourceType     データソース種別 (Table/Query)
0x0068  OnKeyDown         キークリック時
0x0069  OnKeyUp           キー解放時
0x006A  OnKeyPress        キー入力時
0x006B  OnMouseDown       マウスボタンクリック時
0x006C  OnMouseUp         マウスボタン解放時
0x006D  OnMouseMove       マウスボタン移動時
0x0073  OnGotFocus        フォーカス取得後
0x0074  OnLostFocus       フォーカス喪失後
0x007E  OnClick           クリック時
0x009C  RecordSource      フォーム/レポートのデータソース (SQL)
0x00A0  FontName          フォームレベルフォント名
0x00DE  OnEnter           フォーカス取得時
0x00DF  OnExit            フォーカス喪失時
0x00E0  OnDblClick        ダブルクリック時
0x00F5  Filter            フィルタ条件
0x010A  LabelType         ラベル種別
0x015A  InputMask         入力マスク
```

### セクション境界

- フォームレベルプロパティとコントロール別プロパティの境界は、バイナリデータ（レイアウト情報等）を挟んで分かれている
- 各コントロールブロックは Name(0x14) プロパティで始まる
- TypeInfo のコントロール数 = Blob 内のコントロールブロック数

## 既知の制約

### パーサーの制約

- **フォームレベル**: Blob 先頭から逐次パースするが、未知の type に遭遇するとそこでパースを中断する。中断以降のフォームレベルプロパティは取得できない。現状確認しているデータでは主要プロパティ（RecordSource, Filter, Caption, FontName 等）は全て中断前に出現しており実用上の問題は未確認
- **コントロールレベル**: フォームレベルパースとは別にバイトスキャンで Name プロパティのパターン `[14 00 0A 00 00 00]` を検出し、各コントロールセクションを個別にパースする。コントロール内で未知の type に遭遇した場合、そのコントロールの残りのプロパティは取得できない
- **レイアウト座標・プリンタ設定**: Blob 内の非プロパティ領域に格納されており、取得対象外
- **Jet3 (Access 97)**: MSysAccessStorage ではなく MSysAccessObjects に格納される異なるフォーマットのため非対応

### prop_id の対応状況

対応済みの prop_id は名前で表示し、未対応のものは `0xXXXX` 形式で数値表示する。新しい prop_id が判明次第、随時追加していく方針。

**対応済み:**

ID      プロパティ名      用途
------  ----------------  ------------------
0x0011  Caption           コントロール表示テキスト
0x0012  ColumnWidths      コンボボックス列幅
0x0014  Name              コントロール名
0x001B  ControlSource     フィールドバインド先
0x0022  FontName          フォント名（コントロールレベル）
0x0026  Format            表示書式
0x005B  RowSource         コンボボックスデータソース (SQL)
0x005D  RowSourceType     データソース種別 (Table/Query)
0x0068  OnKeyDown         キークリック時
0x0069  OnKeyUp           キー解放時
0x006A  OnKeyPress        キー入力時
0x006B  OnMouseDown       マウスボタンクリック時
0x006C  OnMouseUp         マウスボタン解放時
0x006D  OnMouseMove       マウスボタン移動時
0x0073  OnGotFocus        フォーカス取得後
0x0074  OnLostFocus       フォーカス喪失後
0x007E  OnClick           クリック時
0x009C  RecordSource      フォーム/レポートのデータソース
0x00A0  FontName          フォームレベルフォント名
0x00DE  OnEnter           フォーカス取得時
0x00DF  OnExit            フォーカス喪失時
0x00E0  OnDblClick        ダブルクリック時
0x00F5  Filter            フィルタ条件
0x010A  LabelType         ラベル種別
0x015A  InputMask         入力マスク

**未対応（出力で頻出する未知 ID の例）:**

ID      出現コンテキスト（推定）
------  --------------------------------
0x0000  コントロール末尾に頻出
0x0013  フォームレベル Bool
0x001D  コントロール内
0x0178  GUID（全コントロールに出現）
0x024A  コントロール内 Short
0x024C  コントロール内 Short
0x0268  色系プロパティ
0x0274  色系プロパティ
0x01CA〜0x01D2  コントロール内 Short（連番で出現）

### type の対応状況

type    名前     データサイズ   対応状況
------  -------  -----------  --------
0x01    Bool     4B           対応済み
0x02    Short    5B           対応済み
0x03    Long     6B           対応済み
0x04    Color    8B           対応済み
0x06    (不明)   6B           対応済み（Long として解釈）
0x08    Double   8B+4B trailer  対応済み
0x09    GUID     16B+4B trailer 対応済み
0x0A    Text     C bytes+4B trailer  対応済み（UTF-16LE）
0x0B    Binary   C bytes+4B trailer  対応済み
0x0C    Memo     C bytes+4B trailer  対応済み（UTF-16LE）
0x05    (不明)   不明         未対応（遭遇時パース中断）
0x07    (不明)   不明         未対応（遭遇時パース中断）

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
