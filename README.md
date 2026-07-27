# rab-browser

**rab** = **R**ust-**A**ccelerated **B**rowser(「lab」に近い発音を意図した命名)

軽量なWebブラウザ。現行の主要ブラウザ(Chrome/Edge/Firefox系)がメモリ・CPUともに重いことへの
不満が動機で、Rustはあくまで「軽量」という目的のための手段。OS標準WebView(macOS=WKWebView)を
`wry`/`tao`で直接制御し、UI(サイドバータブ・コマンドパレット)はSolid.js製の別WebViewとして
実装している。将来的にはMCP経由で機能を選択的に有効化できるプラグイン機構を持たせる予定。

設計判断の詳細・技術選定の根拠・フェーズ計画は [`docs/architecture.md`](./docs/architecture.md) を参照。

## 状態

- **Phase 0**: 技術検証スパイク(wry+tao のマルチWebView合成、rmcp最小サーバー) — 完了
- **Phase 1**: 最小WebView表示ブラウザ(単一タブ、URL入力、devtools) — 完了
- **Phase 2**: 縦型サイドバータブUI・複数タブ管理・コマンドパレット(Zen/Arc風) — 完了
- **Phase 3**: MCP機能拡張基盤(ブラウザ自身がMCPサーバーになる) — 進行中
- 既知の未解決事項: パスキー(WebAuthn)がWKWebView上で失敗する問題( [#9](https://github.com/noir-chat-9661/rab-browser/issues/9) )。署名済み`.app`バンドル化が必要な可能性が高く、後回しにしている

## 構成

```
rab-browser/
├── crates/
│   ├── browser-core/         # Tab/TabManager, BrowserEngine trait(GUI非依存)
│   ├── browser-engine-wry/   # wry/tao によるWebViewエンジン実装
│   └── browser-app/          # バイナリ本体。ウィンドウ・クロームWebView・IPC統合
├── ui-chrome/                # サイドバー・タブUI・コマンドパレット(Solid.js + Vite)
├── spikes/                   # Phase 0の技術検証用の使い捨てコード
└── docs/architecture.md      # 設計計画・技術選定の記録
```

## 開発

前提: Rust(edition 2024)、[pnpm](https://pnpm.io/)。macOS(WKWebView)を主眼に開発している。

```bash
# クロームUI(サイドバー等)をビルド。browser-appはこのビルド成果物(ui-chrome/dist/index.html)を
# 実行時に読み込むため、コード変更後は毎回ビルドし直す必要がある
pnpm --dir ui-chrome install
pnpm --dir ui-chrome build

# ブラウザ本体を起動(引数で初期URLを指定可能)
cargo run -p browser-app -- https://example.com
```

### 主なキーボードショートカット

| ショートカット | 動作 |
|---|---|
| `Cmd+L` (macOS) / `Ctrl+L` (Windows/Linux) | コマンドパレットを開く(URL入力) |
| `Cmd+T` (macOS) / `Ctrl+T` (Windows/Linux) | 新規タブを作成 |
| `Cmd+W` (macOS) / `Ctrl+W` (Windows/Linux) | 現在のタブを閉じる |
| `Cmd+R` (macOS) / `Ctrl+R` (Windows/Linux) | 現在のタブをリロード |
| `Cmd+Option+I` (macOS) / `Ctrl+Alt+I` (Windows/Linux) | 開発者ツール(Web Inspector)を開く |
| `Cmd+クリック` (macOS) / `Ctrl+クリック` (Windows/Linux) | リンクを新規タブで開く |

### 開発者ツール

コンテンツ・クローム両方のWebViewで開発者ツールが有効になっている。macOSでは
`Cmd+Option+I`、Windows/Linuxでは`Ctrl+Alt+I`、または
右クリックの「要素を検証」から開ける。

## テスト・Lint

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir ui-chrome build   # tsc --noEmit + vite build
```

## Git運用

- `main`への直接コミットは行わない。作業は `git worktree` で `.worktrees/<name>`(gitignore対象)配下に
  ブランチを切ってから行う
- Issue単位でタスクを分解し、実装後はPRを作成してレビュー・マージする
