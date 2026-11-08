# リポジトリの作業ガイド

## ビルド・テスト・Lint

- ビルド: `cargo build --locked`
- CLIの実行: `TARGET_URL=https://your-app.example.com/ cargo run --locked`
- フォーマット確認: `cargo fmt --check`
- Lint: `cargo clippy --locked --all-targets -- -D warnings`
- テスト: `cargo test --locked`
- テストを名前・フィルターで1件実行: `cargo test --locked <test_filter>`。テストは `src/main.rs` の単体テストにあります。
- コンテナのビルド: `docker build -t page-warmup .`

依存関係を変更した場合は `Cargo.lock` を更新し、再現可能なビルドのため `--locked` を使ってください。

## アーキテクチャ

`src/main.rs` がCLIとウォームアップ確認処理を担います。`TARGET_URL` を必須入力とし、`headless_chrome` でChromiumを起動します。CDPの `Network` と `Page` イベントからドキュメント応答と `DOMContentLoaded` を取得します。ウォームアップにはブラウザー遷移が必要なため、直接HTTPクライアントによるリクエストに置き換えないでください。最初の遷移でウォームアップを促し、20秒後から5秒間隔で、合計180秒の期限まで確認します。

最終応答はJSONとしてstdoutに出力し、進行状況やエラーはstderrに出力します。HTTPステータスが500未満で、`READINESS_HEADER` で指定したヘッダーが `NOT_READY_VALUE`（既定 `asleep`）でなければ応答済みと判定します。`READINESS_HEADER` が未設定の場合はステータスだけで判定するため、4xx応答でも応答済みと扱われる場合があります。

`.github/workflows/page-warmup.yml` とローカル実行は、同じDocker Composeサービスを使ってCLIを実行します。Workflowの手動実行では `target_url` を必須入力とし、必要に応じて `readiness_header` と `not_ready_value` を指定します。WorkflowはBuildxのGitHub Actions cacheを使ってx64イメージをビルドし、手動実行とheaded実行ではXvfb上のheaded Google Chrome Stableを利用します。`.github/workflows/ci.yml` はmainとgitタグのpushでテストを実行し、成功後に `linux/amd64` イメージをGHCRへ公開します（mainは `latest`、gitタグは同名タグ）。`Dockerfile` はRustバイナリを別ステージでビルドし、Google Chrome Stableを非rootユーザーで実行します。`docker-entrypoint.sh` はheadedモードのときXvfbを起動し、`compose.yaml` はChromeが必要とするnamespaceをコンテナで利用できるよう `SYS_ADMIN` を追加します。

## プロジェクト固有の規約

- `TARGET_URL` は必須です。`HEADLESS=true` の場合だけheadlessになり、未設定または `false` ではheadedモードです。
- 一部の環境ではheadlessのブラウザー遷移ではウォームアップしないため、実ターゲットへの実行はheadedを前提にしてください。`HEADLESS=true` はローカル確認用です。
- `READINESS_HEADER` は任意です。設定すると、その名前のレスポンスヘッダーが `NOT_READY_VALUE`（既定 `asleep`）の間は応答済みと判定しません。未設定時はHTTPステータスだけで判定します。
- `NOT_READY_VALUE` は任意です。readiness ヘッダーがこの値の間は未応答と判定します。既定は `asleep` です。
- `CHROME` でChromium実行ファイルを指定できます。標準の場所にない環境では設定してください。
- ウォームアップ確認はブラウザー遷移で行い、応答ステータスとreadiness値はCDPイベントから取得してください。`curl` やHTTP pollingに置き換えないでください。
- 呼び出し側が利用するstdoutのJSON形式を維持し、運用ログはstderrまたは `RUST_LOG` 経由にしてください。
- Chromiumのsandboxを有効に保ってください。Dockerイメージではsetuid sandbox helperを設定済みです。`--no-sandbox` や `no-new-privileges` を追加しないでください。
- タイムアウトと再試行間隔は `src/main.rs` 冒頭の定数で管理してください。
