# broadcast-pane

[English](README.md)

[Herdr](https://herdr.dev) で、打ったキーをタブ内の全ペイン（または選んだペイン）へ同時に送るプラグインです。
使い勝手は tmux の `synchronize-panes` に近いものです。

![broadcast-pane のデモ](assets/demo.gif)

キーを押すと、画面中央に小さなポップアップが開きます。
ポップアップで打ったキーは、同じタブのペイン全部（または選んだペイン）へそのまま届きます。
レイアウトは変わらず、送り先のペインはポップアップの周りに見えたままです。

```
┌─ pane 1 ──────────┬─ pane 2 ──────────┐
│ $ uptime          │ $ uptime          │
│  10:12 up 3d      │  10:12 up 9d      │
│ ┌─ Broadcast ───────────────────────┐ │
│ │ broadcast → 2/2 (1 web, 2 api)  … │ │
│ │   ls⏎                             │ │
│ │ › echo hi                         │ │
│ └───────────────────────────────────┘ │
└───────────────────┴───────────────────┘
```

- **入力系のキーだけ転送**: 文字（日本語入力の確定文字も含む）・`Enter`・`Backspace`・`Tab`・`Ctrl+c`・
  `Alt+Enter`、行編集（`Ctrl+a`/`e`/`b`/`f`/`k`/`u`/`w`、左右キー、`Home`/`End`/`Del`）を届けます。
  `Esc`・`Ctrl+d`・`Ctrl+l`・`Ctrl+r`・`Ctrl+z`・`Alt+キー` などそれ以外のショートカットは送りません。
- **送り先**: 最初は、ポップアップを開いたときの同じタブのペイン全部（開く前にいたペインも含む）。
  途中で閉じたペインは自動で外れ、全部なくなるとポップアップも閉じます。
- **送り先を選ぶ**: `Ctrl+]` で、入力用のポップアップが送り先選択用のポップアップに切り替わります。
  タブのペインを 1 行に 1 つ、送り先なら `●`、外したら `○` で一覧表示します
  （高さはペインの数に合わせて画面の半分まで。入りきらなければスクロール）。
  上下キー・`Ctrl+p`/`Ctrl+n`・`j`/`k` で移動、`Space` で送る／送らないを切り替え、
  数字キー（1〜9）でそのペインを直接切り替え、`a` で全部選択（全部選択済みなら全部解除）、
  `Enter`・`Esc`・`Ctrl+]` で入力用のポップアップに戻ります（入力中の行もそのまま戻ります）。
  番号は画面上の位置順（上から、同じ高さなら左から）で、名前は `prefix+P` で付けたペイン名
  （なければ端末タイトル、agent 名、cwd の末尾）、続けて agent 名と cwd を表示します。
  選択はポップアップを閉じるまで有効で、次に開いたときは全ペインに戻ります。
- **打ったキーを表示**: ポップアップにも入力中の行と直前の行を表示します。
  `Ctrl+a`/`Ctrl+e`/`Ctrl+k`/`Ctrl+u`/`Ctrl+w`、左右キーなどの行編集は表示にも反映し、
  `Alt+Enter` の改行後は、カーソルのある行を表示します（`Ctrl+p`/`Ctrl+n`、上下キーで行を移動）。
  `Tab` は `‹Tab›` と表示します。送り先のシェルの状態（補完や履歴の
  中身）は分からないので、表示はあくまで打ったキーの記録です。
- **履歴は呼び出さない**: 上下に移動する行がないときの `Ctrl+p`/`Ctrl+n`、上下キーは送りません。
- **`Ctrl+g`・`Ctrl+q` で終了**: ポップアップが閉じます。

## 制限

- Herdr のプラグインは通常のペインへの打鍵を横取りできないため、入力は専用のポップアップで行います。
- ポップアップは位置を指定できず画面中央に出るため、送り先のペインの中央付近が隠れます。
- Herdr はポップアップを 1 つしか開けず大きさも変えられないため、送り先の選択は入力用のポップアップを
  閉じて開き直す形になります。切り替えの一瞬に打ったキーは失われます。
- ポップアップを開いている間は、Herdr の prefix キーもそのまま送り先へ転送されます（prefix コマンドは使えません）。
- ポップアップを開いた後に増やしたペインには送りません（`Ctrl+]` で送り先の選択を開くと一覧に加わります）。
- 入力系以外のキー（`Esc`・`Ctrl+d` など）は転送しないので、vim や top などの操作には使えません。
- `Ctrl+g`・`Ctrl+q`・`Ctrl+]` は転送できません。上下に移動する行がないときの上下キーも転送しません。

## 必要なもの

- Herdr 0.9.2 以降
- macOS または Linux
- ビルドに Rust 1.85 以降（`cargo`）

## インストール

```sh
herdr plugin install abroller666/broadcast-pane
```

インストール時に `scripts/build.sh`（`cargo build --release`）が実行され、`bin/broadcast-pane` が作られます。

`~/.config/herdr/config.toml` にキーを割り当てます。

```toml
[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "abroller666.broadcast-pane.open"
description = "broadcast to all panes"
```

`herdr server reload-config` で反映します。

## 開発

ローカルの clone を使う場合は、ビルドしてから link します（`herdr plugin link` はビルドしません）。

```sh
git clone https://github.com/abroller666/broadcast-pane.git
cd broadcast-pane
sh scripts/build.sh
herdr plugin link .
```

```sh
cargo test
cargo clippy --all-targets
sh scripts/build.sh   # 変更後は再ビルドすると、次に開いたポップアップから反映される
```

## ライセンス

MIT
