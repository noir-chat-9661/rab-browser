# rab-browser 設計計画

rab = **R**ust-**A**ccelerated **B**rowser（"lab" に近い発音を意図した命名）

軽量なWebブラウザをRustで作る。現行の主要ブラウザ(Chrome/Edge/Firefox系)がメモリ・CPUともに
重いことへの不満が動機。「軽量」が真の目的であり、Rustはその手段。

計画立案: Claude Opus 5 (Agent tool `model: opus`, subagent_type: `planner`)
参照した既存ファイル: `~/.agents/skills/browser-mcp/SKILL.md`, `~/.agents/skills/browser-mcp/references/tools.md`

## 0. 前提修正(一次情報による検証結果)

- **Verso(Servoベースのブラウザ)は2026年時点でメンテナンス終了・アーカイブ済み**。Servo本体の更新に
  追従できず開発停止。「真にRust製エンジン」路線はMVPの土台にできない。
- Servo本体は開発継続中だが、ブラウザとして使えるのは `servoshell`(winit + egui のデモ)レベルで、
  Web互換性は日常利用に耐えない。埋め込み(embedding API)も発展途上。
- **wry/tao(Tauriの下回り)は成熟**しており、OS標準WebView(macOS=WKWebView / Windows=WebView2 /
  Linux=WebKitGTK)を使うことで Electron比でRAM約1/5、バイナリ数MB。
- **rmcp(公式Rust MCP SDK)** はcrates.ioで実績多数・stdioトランスポート標準対応。

## 1. レンダリング方式のトレードオフと推奨

| 方式 | 起動速度 | メモリ | Web互換性 | 保守コスト | 実装難易度 | 「軽量」目的への適合 |
|---|---|---|---|---|---|---|
| **wry/tao(システムWebView)** ← 推奨 | 速い | 最小(OS共有) | 高 | 低 | 中 | ◎ |
| Servo(servoshell/embedding) | 中 | 中〜低 | 低〜中(実用未達) | 高 | 高 | △(将来の夢) |
| Verso | — | — | — | プロジェクト消滅 | — | ✕ |
| Chromium(CEF/Electron) | 遅い | 大 | 最高 | 中 | 低 | ✕(重い問題の再発) |

**推奨: `wry` + `tao` を直接使う(フルTauriは使わない)**

- フルTauriは「1アプリ=主に1 WebView」のアプリ配布フレームワークで、Nタブ=N WebViewを動的生成・破棄する
  “ブラウザ”用途にはやや逆向き。wryを直接握れば「コンテンツWebView群 + クローム用WebView」を自前で
  コンポジットできる。Tauri v2のマルチWebView機能(unstable)で足場を組む選択肢もPhase 0で比較検討。
- レンダリングは `trait BrowserEngine` で抽象化し、初期実装を `WryEngine` にする。Servo成熟後に
  `ServoEngine` を追加できる構造にしておく。

### この選択に伴う制約

システムWebViewを使うため、既存 `browser-mcp`(Electron+Chromium+CDP)でできていた一部がそのままは
実現困難:
- 実現容易: navigate / click / type / screenshot / get_dom / get_text / evaluate / cookies /
  localStorage / タブ操作 / CTF系(DOM由来はJS注入で導出可能)
- 困難・限定的: `browser_get_network_requests`(CDP相当のネットワーク傍受)。WKWebViewでは
  全リクエストログ取得が限定的 → MVPでは「主要リソースのみ/ベストエフォート」と割り切る。

## 2. GUIフレームワーク(クローム)の方針

**クローム(サイドバー・タブ・コマンドパレット)も専用WebViewで描画し、Rust側とIPCで接続。**

- egui/iced/gpui等のネイティブGUIとwry WebViewを1ウィンドウ内で合成するのは難易度が高い。
  クロームもWebViewにすれば合成が単純(クロームWebViewを最前面、コンテンツWebViewをその内側に配置)。
- クロームのフロントは軽量重視で **Solid.js または Svelte + Vite**。React/Next相当のヘビースタックは
  軽量目的に反するため不採用。
- IPC: wryの `ipc_handler` / `custom protocol` でRust↔クローム間をJSONメッセージング。

## 3. ディレクトリ構成

```
rab-browser/
├── Cargo.toml                # [workspace]
├── crates/
│   ├── browser-core/         # BrowserEngine trait, タブ/履歴/セッションモデル(GUI非依存)
│   ├── browser-engine-wry/   # WryEngine: wry/tao でWebView生成・レイアウト・イベント
│   ├── browser-app/          # バイナリ本体。ウィンドウ・クロームWebView・IPC統合
│   ├── browser-mcp-server/   # rmcp ベース。coreを操作してMCPツールを公開
│   └── browser-plugins/      # 機能モジュール(feature単位)のレジストリ+各モジュール
├── base-ui/                # クロームのフロント(Solid/Svelte + Vite)
├── skills/                   # browser-mcp skill構造を踏襲した機能ドキュメント群
│   └── <feature-name>/{SKILL.md, references/, scripts/}
├── docs/
│   └── architecture.md       # 本ファイル
└── README.md
```

## 4. 「ブラウザ自身がMCPサーバー」設計

- 既存 `~/.agents/skills/browser-mcp` はElectron製の別プロセスをheadlessで動かす自動化専用サーバー
  (37ツール)。新ブラウザはユーザーが実際に見ているGUIタブをそのまま操作対象にできる点が新しい。
- 2つの運用モード:
  - **Attachedモード**: 起動中GUIブラウザに対しMCPクライアントが接続し、ユーザーが見ている実タブを操作。
  - **Headlessモード**: GUIなしでMCPサーバーのみ起動(既存browser-mcpの置き換え用途)。
- **ツール互換**: 既存37ツールと同名・同シグネチャを極力踏襲(navigate/click/type/screenshot/get_dom/
  cookies/tabs/ctf_*)。network系のみ「限定対応」と明記。
- **プラグイン=MCPツールモジュール**: `browser-plugins` に
  `trait FeatureModule { fn tools(&self) -> Vec<ToolDef>; fn enabled(&self) -> bool; }` を定義。
  設定ファイル(`~/.config/rab-browser/features.toml`)で有効化フラグを持ち、起動時にレジストリが
  有効モジュールのツールだけをrmcpに登録 = 「MCPで選択したら展開できる機能」の実体。
- 各機能モジュールは `skills/<feature>/SKILL.md + references/ + scripts/` を同梱。

## 5. UI設計方針(Zen/Arc要素の線引き)

- **P0(MVP必須)**: 縦型サイドバータブ、最小クローム、コンテンツ領域最大化。
- **P1**: コマンドパレット(Cmd+L / Cmd+T でURL入力・タブ検索・アクション実行)。
- **P2**: Spaces/Workspaces(タブ群の切り替え)。
- **P3**: スプリットビュー、ピン留めタブ、テーマ。

## 6. フェーズ分け

### Phase 0: 技術検証スパイク(捨ててよいコード)
- wry + tao で1ウィンドウ・1 WebViewを表示し `https://example.com` をロードする最小バイナリ
- 同一ウィンドウ内に2つ目のWebView(クローム用)を重ねて配置できるか検証
- macOS WKWebViewでWebViewを動的に生成/破棄してもleakしないことをメモリ計測で確認
- rmcpでstdioの最小MCPサーバー(pingツールのみ)を起動しClaude Codeから接続確認
- スパイク結果で「生wry+tao」か「Tauri v2 multiwebview(unstable)」かを最終決定

### Phase 1: 最小WebView表示ブラウザ
- `browser-core` に Tab/TabId/TabManager/BrowserEngine trait
- `browser-engine-wry` にWryEngine(WebView生成・URLロード・戻る/進む/リロード・矩形配置)
- `browser-app` でウィンドウ起動→単一タブ表示→URL直接指定でナビゲート
- キーボードショートカット最小(Cmd+L=URL入力、Cmd+R=リロード)

### Phase 2: タブ / Zen・Arc風UI
- `base-ui` を Solid(または Svelte)+ Vite でセットアップ
- クロームWebView ↔ Rust の IPCプロトコル
- 縦型サイドバータブUI、複数コンテンツWebViewの生成・破棄・表示切替
- コマンドパレット、履歴(戻る/進む)

### Phase 3: MCP機能拡張基盤
- `browser-mcp-server` を rmcp で実装
- Attached/Headless 2モード
- コアツール移植(既存browser-mcpと同名)
- `browser_get_network_requests` は「限定対応」
- `FeatureModule` trait とレジストリ、`features.toml`
- CTF系ツールを1プラグインモジュールとして実装
- `skills/<feature>/SKILL.md` テンプレート

### Phase 4: Zen/Arc体験の拡充
- Spaces/Workspaces、ピン留めタブ・タブ永続化、スプリットビュー、テーマ

## 7. リスク・不確実性

- 【中〜高】マルチWebViewの合成品質(リサイズ追従・入力フォーカス・zオーダーがOS依存) → Phase 0最優先
- 【中】MCPツールのシステムWebView制約(ネットワークログ・CDP相当の内省が弱い)
- 【低〜中】Servo将来採用(BrowserEngine抽象で差し替え口だけ残す)
- 【低】クロームフロントの肥大化(Solid/Svelteに固定)

## Phase 0 検証結果

### 実装したスパイク

- `spikes/webview-compose`: `tao` で1ウィンドウを作り、`wry` のコンテンツWebViewで
  `https://example.com` をロードする最小バイナリ。
- 同じ親ウィンドウに2つ目のWebViewを子ビューとして生成し、インラインHTMLで仮クロームを
  表示。コンテンツビューはウィンドウ全体、クロームビューは上端72pxに重ねている。
- `spikes/mcp-ping`: `rmcp` の stdio トランスポートに `ping` ツールだけを登録する最小サーバー。

### 観察結果

- 2つのWebViewは同一ウィンドウ内のネイティブ子ビューとして生成できた。後から生成した
  クロームWebViewが前面に来るため、zオーダーの基本要件は満たせる。
- ウィンドウのリサイズイベントで両方の `set_bounds` を呼ぶことで追従できる。クロームの
  入力は前面のクロームWebViewが受け、コンテンツ側へフォーカスを戻すには明示的な
  フォーカス制御が必要になる。全画面透明オーバーレイを常用すると、コンテンツのクリックを
  クロームが遮るため、実装ではクロームの矩形を必要な範囲に限定すべきである。
- 起動時に一時WebViewを生成→破棄する処理を3回実行するライフサイクル・スモークテストを
  入れた。ただし、この検証環境はGUIセッションを持たず、実行時は `NSScreen` が取得できず
  taoのウィンドウ生成前に停止したため、macOSでの目視確認は未実施。GUIのあるmacOS上で
  起動して、異常終了や明らかなリークがないことを確認する必要がある（厳密なプロファイルではない）。
- `cargo build --workspace` は通過した。

### Phase 0 の判断

API上は `wry + tao` の直接利用でコンテンツWebViewとクロームWebViewを同一親ウィンドウの
子ビューとして合成できる構成になっており、コンパイルも通過した。次フェーズは生 `wry + tao` で進める。
Tauri v2のmultiwebview機能は、将来OS差分やライフサイクル管理で問題が出た場合の比較対象として
残すが、現時点で切り替える理由はない。

### 実機(GUIあり)での追加確認

Codexのサンドボックスにはディスプレイセッションがなく上記の目視確認は未実施だったため、
実ディスプレイのあるmacOS (M4, macOS 27) 上でClaude側が追加確認した:

- `./target/debug/webview-compose-spike` を起動し、プロセスが4秒以上クラッシュせず生存し
  続けることを確認(=タブビューの生成・破棄ループやウィンドウ生成そのものは異常終了しない)。
- ただし、この検証セッションには画面収録/アクセシビリティ権限がなく、`screencapture`と
  Accessibility API経由でのウィンドウ内容の目視確認は取得できなかった。**チェックリストの
  「クローム/コンテンツの重なり・zオーダー・リサイズ追従が見た目通りか」は依然、人間が
  実際に画面を見て確認する必要がある。**
- 結論: クラッシュしないことは実機で確認済み。見た目の合成品質(最大のリスク項目)は
  未確認のまま。Phase 1着手前に、ユーザー自身が一度 `cargo run -p webview-compose-spike`
  を実行して目視確認することを推奨する。

## 実装状況

- **Phase 0**: 技術検証スパイク — 完了
- **Phase 1**: 最小WebView表示ブラウザ — 完了
- **Phase 2**: タブ / Zen・Arc風UI — 完了
- **Phase 3**: MCP機能拡張基盤 — 完了
- **Phase 4**: Zen/Arc体験の拡充(Spaces/Workspaces等) — 未着手
- **Windows対応**: `cargo build`/`cargo test`はCI(windows-latest)で確認済み。
  ネイティブメニューバー( [#95](https://github.com/noir-chat-9661/rab-browser/issues/95) )・
  JSダイアログ( [#96](https://github.com/noir-chat-9661/rab-browser/issues/96) )の実機動作は
  未検証。Linuxは未着手
- 既知の未解決事項: パスキー(WebAuthn)がWKWebView上で失敗する問題
  ( [#9](https://github.com/noir-chat-9661/rab-browser/issues/9) )。署名済み`.app`
  バンドル化が必要な可能性が高く、後回しにしている

## ディレクトリ構成(実態)

```
rab-browser/
├── crates/
│   ├── browser-core/         # Tab/TabManager, BrowserEngine trait(GUI非依存)
│   ├── browser-engine-wry/   # wry/tao によるWebViewエンジン実装
│   ├── browser-app/          # バイナリ本体。ウィンドウ・クロームWebView・IPC統合
│   └── browser-mcp-server/   # rmcpベースのMCPサーバー実装
├── base-ui/                # サイドバー・タブUI・コマンドパレット(Solid.js + Vite)
├── spikes/                   # Phase 0の技術検証用の使い捨てコード
├── skills/rab-browser-mcp/   # MCPツールの使い方をまとめたAIエージェント向けskill
├── scripts/build-app.sh      # .appバンドルのビルドスクリプト
└── docs/architecture.md      # 本ファイル
```

`browser-plugins`(FeatureModule方式のプラグインレジストリ)は計画段階で、まだ実装されていない。

## 開発

前提: Rust(edition 2024)、[pnpm](https://pnpm.io/)。macOS(WKWebView)を主眼に開発している。

```bash
cargo build --workspace   # base-ui/dist も自動ビルドされる
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Git運用

- `main`への直接コミットは行わない。作業は `git worktree` で `.worktrees/<name>`(gitignore対象)配下に
  ブランチを切ってから行う
- Issue単位でタスクを分解し、実装後はPRを作成してレビュー・マージする
