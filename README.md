# rab-browser

**rab** = **R**ust-**A**ccelerated **B**rowser(「lab」に近い発音を意図した命名)

軽量なWebブラウザ。現行の主要ブラウザ(Chrome/Edge/Firefox系)がメモリ・CPUともに重いことへの
不満が動機で、Rustはあくまで「軽量」という目的のための手段。OS標準WebView(macOS=WKWebView)を
`wry`/`tao`で直接制御し、UI(サイドバータブ・コマンドパレット)はSolid.js製の別WebViewとして
実装している。

現状**macOS専用**。設計判断の詳細・技術選定の根拠・開発フェーズの記録は
[`docs/architecture.md`](./docs/architecture.md) を参照。

**現在ベータ版。** ビルド済み`.app`は [Releases](https://github.com/noir-chat-9661/rab-browser/releases)
から入手できる。ad-hoc署名のみのため、初回起動時はGatekeeperの警告が出る
(右クリック→「開く」で許可するか、`xattr -d com.apple.quarantine rab-browser.app` で
quarantine属性を削除する)。

## 使い方

前提: macOS、Rust(edition 2024)、[pnpm](https://pnpm.io/)。

```bash
# ブラウザ本体を起動(引数で初期URLを指定可能)。crates/browser-app/build.rs が
# cargo build のたびに base-ui/dist/index.html を自動ビルドするため、
# 手動で pnpm build を叩く必要はない(pnpmがPATHに無い場合はwarningを出して
# スキップするので、その場合のみ手動で `pnpm --dir base-ui install && pnpm --dir base-ui build` を実行する)
cargo run -p browser-app -- https://example.com
```

### `.app`バンドルとしてビルドする

```bash
./scripts/build-app.sh
```

`target/release/rab-browser.app` が生成される(Finderからダブルクリック起動可能)。
ad-hoc署名(`codesign --sign -`)のみを行っており、配布用のDeveloper ID署名・
notarizationは未対応。ローカルでの動作確認用(パスキー/WebAuthn関連の検証など)に使う。

### 主なキーボードショートカット

| ショートカット | 動作 |
|---|---|
| `Cmd+L` | コマンドパレットを開く(URL入力) |
| `Cmd+T` | 新規タブを作成 |
| `Cmd+W` | 現在のタブを閉じる |
| `Cmd+R` | 現在のタブをリロード |
| `Cmd+[` / `Cmd+]` | 戻る / 進む |
| `F12` / `Cmd+Option+I` | 開発者ツール(Web Inspector)を開く |
| `Cmd+クリック` | リンクを新規タブで開く |

## 機能

- **サイドバータブUI**: Zen/Arc風の縦型サイドバー。タブの新規作成・切り替え・クローズ、
  favicon・タイトル表示に対応
- **お気に入り(ブックマーク)**: 現在のページのトグル追加・削除、一覧表示
- **閲覧履歴**: セッション内の閲覧履歴を保持(最大200件、直前と同一URLへの連続遷移は記録しない)
- **コマンドパレット**: `Cmd+L`/`Ctrl+L`でURL入力・検索クエリの実行(デフォルト検索エンジンは
  Google/DuckDuckGo/Bingから設定パネルで選択可能)
- **戻る/進む・リロード**: `Cmd+[`/`Cmd+]`(矢印キー相当)や `Cmd+R` に対応
- **開発者ツール**: コンテンツ・クローム両方のWebViewで有効(詳細は下記)
- **テーマ・言語設定**: ダーク/ライトテーマ、日本語/英語表示の切り替え
- **プライバシー**: 履歴・Cookie/閲覧データの削除
- **MCPサーバー内蔵**: ブラウザ自身がMCPサーバーとして動作し、AIアシスタントからタブ操作・
  ページ内容取得・クリック/入力などを実行できる。Claude Desktop / Claude Code CLI / Cursor /
  Windsurf / Cline / Antigravity / Zed / Codex CLI / OpenCode CLI へは設定パネルから
  ワンクリックで登録可能(詳細は下記)

## パフォーマンス

- **軽量WebViewエンジン**: Electron等の同梱Chromiumではなく、OS標準WebView(macOS=WKWebView)を
  `wry`/`tao`で直接制御することで、ブラウザ本体のバイナリサイズ・起動コストを抑えている
- **非アクティブタブのサスペンド**: バックグラウンドタブを一定時間(デフォルト5分、設定パネルの
  「パフォーマンス」カテゴリで10秒〜1時間の範囲でカスタマイズ可能)放置すると、裏側のWKWebViewを
  破棄してメモリを解放する。再生中の音声/動画があるタブは対象外。次に開いたときは同じURLを
  自動的に再読み込みする
  - 猶予期間を後から延長しても、既にサスペンド済みのタブが復活することはない(一方向の破棄)
- **タブ切り替え中心のUXに最適化**: 頻繁に切り替えながら作業するタブが毎回破棄されないよう、
  猶予期間という形でグレースピリオドを設けている

### 開発者ツール

コンテンツ・クローム両方のWebViewで開発者ツールが有効になっている。`F12`、macOSでは
`Cmd+Option+I`、Windows/Linuxでは`Ctrl+Alt+I`、または
右クリックの「要素を検証」から開ける。

### MCPサーバー

MCPは**自動では有効化されない**。以下のいずれかを明示的に行う必要がある。

- 起動時に`--mcp`フラグを付けるか、環境変数`RAB_MCP=1`を設定する(標準入出力/stdio方式)
- 設定パネルのMCPカテゴリでStreamable HTTPを有効にする
  (`http://127.0.0.1:8765/mcp`で待ち受け。ポート番号は設定パネルで変更可能)

両方式は独立しており、同時に利用できる。

#### MCPクライアントへの登録

設定パネルのMCPカテゴリにある「登録」ボタンから、対象クライアントを選んでワンクリックで
それぞれの設定ファイルへ追記できる(既存の他エントリ・他の設定項目は保持される)。

| クライアント | 設定ファイル |
|---|---|
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Claude Code CLI | `~/.claude.json` |
| Cursor | `~/.cursor/mcp.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |
| Cline(VS Code拡張) | `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` |
| Antigravity(agy CLI) | `~/.gemini/config/mcp_config.json` |
| Zed | `~/.config/zed/settings.json`(`context_servers`キー) |
| Codex CLI | `~/.codex/config.toml`(`mcp_servers`テーブル) |
| OpenCode CLI | `~/.config/opencode/opencode.json`(`mcp`キー) |

それ以外のMCPクライアントは、上記いずれかの設定ファイルを参考に手動でstdio方式のサーバーとして
登録すること。

Streamable HTTPはローカルホストのみにbindし、DNS rebinding対策としてHostヘッダーも
loopbackホストに制限している。ただし認証はないため、同じマシン上のすべてのプロセスが
ブラウザを操作できる。信頼できないプロセスが動作している環境では有効にしないこと。

## 開発への貢献

ビルド構成・テスト/Lintコマンド・ブランチ運用など開発者向けの情報は
[`docs/architecture.md`](./docs/architecture.md) を参照。

## ライセンス

[MIT license](LICENSE-MIT) と [Apache License, Version 2.0](LICENSE-APACHE) のデュアルライセンスで公開している。
どちらか一方の条件を選んで利用できる。
