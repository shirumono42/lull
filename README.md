# Lull

ショートカットキー一発で鳴らせる、常駐型の小さなノイズ＆バイノーラルビート生成アプリです。
Windows と macOS で動きます。Rust 製で、待機中のメモリ使用量は約 2 MB です。

<img src="docs/screenshot.png" width="300" alt="Lull のパネル">

## ダウンロード

[Releases](https://github.com/shirumono42/lull/releases/latest) から入手してください。

| OS | ファイル | 使い方 |
| --- | --- | --- |
| Windows 10/11 (x64) | `Lull-windows-x64.zip` | 展開して `Lull.exe` を実行 |
| macOS 11+ (Apple Silicon / Intel) | `Lull-macos-universal.zip` | 展開して `Lull.app` を「アプリケーション」へ移動して起動 |

インストーラーはありません。起動するとタスクトレイ（macOS はメニューバー）にアイコンが出ます。

> **macOS で「開発元を確認できない」と表示されたら**: 署名していないためです。
> `Lull.app` を右クリック →「開く」を選ぶか、ターミナルで次を実行してください。
>
> ```bash
> xattr -dr com.apple.quarantine /Applications/Lull.app
> ```
>
> **Windows で SmartScreen が出たら**: 「詳細情報」→「実行」を押してください。

## 使い方

| 操作 | Windows | macOS |
| --- | --- | --- |
| 再生 / 停止 | `Ctrl + Alt + N` | `Control + Option + N` |
| パネルを開く / 閉じる | `Ctrl + Alt + L` | `Control + Option + L` |
| パネルを開く | トレイアイコンを左クリック | メニューバーのアイコンをクリック |
| 終了 | トレイアイコンを右クリック → Quit Lull | アイコンを右クリック → Quit Lull |

パネルの外をクリックするとパネルは消えますが、音は鳴り続けます。パネル表示中は
`Space` で再生 / 停止、`Esc` で閉じる、`Q` で終了できます。

### パネルでできること

- **Noise**: ホワイト / ピンク / ブラウンノイズを個別にオン・オフし、音量を調整。複数を同時に重ねられます。
- **Binaural Beats**: オン・オフと音量、プリセット（δ Delta 2 Hz / θ Theta 6 Hz / α Alpha 10 Hz / β Beta 20 Hz）。
  - **Beat**: 左右の周波数差（0.5〜40 Hz）を自由に調整
  - **Tone**: 基準となる音の高さ（60〜600 Hz）
- **Master**: 全体の音量
- スライダーはドラッグのほか、マウスホイールでも動かせます。
- ヘッダー右の色付きバーで、いま鳴っている層がひと目でわかります。

バイノーラルビートは左右の耳に別の周波数を流すため、**ヘッドホン / イヤホンで聴いてください**。

設定は自動で保存されます（パネルを閉じたときと終了時）。

- Windows: `%APPDATA%\lull\settings.txt`
- macOS: `~/Library/Application Support/lull/settings.txt`

### ショートカットキーの変更

他のアプリとキーが重なっている場合、パネル下部に `(in use)` と表示されます。
`settings.txt` の次の行を書き換えて Lull を再起動してください。

```
hotkey_play=ctrl+alt+KeyN
hotkey_panel=ctrl+alt+KeyL
```

修飾キーは `ctrl` `alt`（macOS では Option）`shift` `super`（Windows キー / Command）、
キーは `KeyA`〜`KeyZ`、`Digit0`〜`Digit9`、`F1`〜`F12`、`Space` などが使えます。

### ログイン時に自動起動する

- Windows: `Win + R` で `shell:startup` を開き、`Lull.exe` のショートカットを置く
- macOS: システム設定 → 一般 → ログイン項目 に `Lull.app` を追加

## 省メモリの工夫

- パネルは表示中だけ存在し、閉じるとウィンドウ・描画バッファ・フォントをすべて破棄します。
- GPU を使わず、CPU で直接描画します（tiny-skia + fontdue）。
- 停止中はオーディオデバイスを解放します。止めるときは短くフェードアウトしてから解放します。
- 音声処理はロックなし（atomic）で、コールバック内でメモリを確保しません。

Windows 11 で計測した値（Private Bytes）は次のとおりです。

| 状態 | メモリ |
| --- | --- |
| 待機中（パネル非表示・停止） | 約 1.8 MB |
| 再生中（パネル非表示） | 約 4.4 MB |
| パネル表示中 | 約 9 MB |

## ソースからビルド

[Rust](https://rustup.rs/) をインストールしてから実行します。

```bash
cargo build --release
```

実行ファイルは `target/release/lull`（Windows は `lull.exe`）に出力されます。

## 技術メモ

- ノイズ: ホワイトは xorshift 乱数、ピンクは Paul Kellet のフィルタ、ブラウンはリーク付き積分。左右は独立に生成するので広がりのある音になります。
- バイノーラル: 左 `Tone − Beat/2`、右 `Tone + Beat/2` の正弦波。
- 音量の変化はサンプル単位でなめらかにしているので、操作してもプチッというノイズが出ません。

## License

MIT
