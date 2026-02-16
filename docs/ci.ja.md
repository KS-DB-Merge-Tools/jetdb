# CI ツール

jetdb プロジェクトで使用する CI ツールとその実行方法。

## ツール一覧

### 1. cargo test — テスト実行

プロジェクト全体のユニットテストと統合テストを実行する。

```bash
cargo test
```

個別クレートのテスト:

```bash
cargo test -p jetdb          # ライブラリのみ
cargo test -p jetdb-cli      # CLI のみ
```

### 2. cargo clippy — リント

Rust の静的解析ツール。警告をエラーとして扱い、コードの品質を保つ。

```bash
cargo clippy -- -D warnings
```

インストール（rustup に含まれていない場合）:

```bash
rustup component add clippy
```

### 3. cargo audit — 脆弱性チェック

依存クレートに既知の脆弱性がないか検査する。

```bash
cargo audit
```

インストール:

```bash
cargo install cargo-audit
```

### 4. cargo doc — ドキュメントビルド

ワークスペース全体の API ドキュメントを生成する。リンク切れや doc comment の構文エラーを検出できる。

```bash
cargo doc --workspace
```

生成されたドキュメントは `target/doc/jetdb/index.html` に出力される。

### 5. rust-code-analysis-cli — 複雑度メトリクス

ソースコードの循環的複雑度・認知的複雑度などのメトリクスを計測する。

```bash
rust-code-analysis-cli -m -p crates/ -O json
```

インストール:

```bash
cargo install rust-code-analysis-cli --locked
```

> **注意**: `--locked` フラグは必須。省略すると tree-sitter のバージョン不一致によりコンパイルが失敗する（[GitHub Issue #1140](https://github.com/nickel-org/rust-code-analysis/issues/1140)）。
>
> このプロジェクトは最終リリースが 2023年1月であり、メンテナンスが停滞している。

### 6. cargo-llvm-cov — テストカバレッジ

LLVM ソースベースのコードカバレッジを使用してテストカバレッジを計測する。

```bash
cargo llvm-cov --workspace
```

HTML レポート:

```bash
cargo llvm-cov --workspace --html
```

HTML レポートは `target/llvm-cov/html/index.html` に出力される。

インストール:

```bash
cargo install cargo-llvm-cov
```

> **注意**: `llvm-tools-preview` コンポーネントが必要。初回実行時に自動でインストールされる。

## 推奨実行順序

1. `cargo test` — まず既存テストが通ることを確認
2. `cargo llvm-cov --workspace` — テストカバレッジを計測
3. `cargo clippy -- -D warnings` — コード品質のチェック
4. `cargo audit` — セキュリティ上の問題がないか確認
5. `cargo doc --workspace` — ドキュメントが正しく生成されるか確認

テストが通らない状態で他のチェックを行っても意味がないため、`cargo test` を最初に実行する。`cargo llvm-cov` はカバレッジ計測のためテストの直後に実行する。`cargo clippy` はコードの問題を検出するため早い段階で実行する。`cargo audit` と `cargo doc` は独立しているため順序は問わない。
