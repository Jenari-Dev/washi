# Notice / ご注意

[English](#english) · [日本語](#日本語)

## English

**Washi is an independent, unofficial open-source project.** It is not affiliated with, endorsed,
sponsored or supported by Canon Inc. or any of its group companies.

### Trademarks
Canon, キヤノン, PIXUS, PIXMA, Canon PRINT and the names of Canon paper products are trademarks or
registered trademarks of Canon Inc. Washi uses these names only to say which printers and paper it
works with. All other trademarks belong to their respective owners.

### What this repository contains — and what it does not
- Everything here is original work: source code, user interface, icons, illustrations, wording and
  the Japanese translation.
- It contains **no** Canon software, firmware, source code, decompiled code, images, icons, fonts,
  screenshots or user-interface text. A release check (`tools/check-vendor-strings` in the
  maintainer's workflow) compares the tree against the vendor's string tables and files before
  every release.
- Washi talks to the printer through open standards — IPP (PWG) for printing, eSCL (Mopria) for
  scanning — and through the printer's own local management interface (CHMP/IVEC), which it
  implements independently.

### How the protocol knowledge was obtained
The management protocol was documented by observing the author's own printer and analysing
software lawfully installed on the author's own computer, solely to make an independent program
work with that printer (interoperability). In Japan, analysing a program in this way is a use "not
for enjoying" the work under Article 30-4 of the Copyright Act (Agency for Cultural Affairs,
*Basic views on the flexible limitation provisions*, 2019, Q12). Only facts needed for
interoperability (message names, element names, value ranges) are published — no vendor code.
The result is written down in [`docs/PROTOCOL.md`](docs/PROTOCOL.md).

### What Washi does not do
- It does not circumvent any access control, copy protection or authentication.
- It does not read, change or bypass ink-cartridge authentication or service counters, and it
  never reads, modifies or replaces the printer's firmware image. The optional firmware update only
  asks the printer to run its own update, which the printer fetches itself; it is unverified on
  real hardware and always asks first.
- It does not include the printer maker's network-setup credentials; Wi-Fi setup of a new printer is
  left to the printer's own panel or WPS.
- It sends no telemetry and contacts nothing but the printer you choose (plus web pages you open
  yourself, such as the printer's manual, after Washi has asked).

### Use at your own risk
Washi is provided **without any warranty** (see the licence). Maintenance functions use ink and
paper; follow the on-screen instructions. Never turn the printer off during a firmware update.

### Licence
GPL-2.0-or-later (see `LICENSE`). Some Rust dependencies are licensed Apache-2.0 only, which is
compatible with GPL version 3; compiled binaries are therefore distributed under the terms of
GPL-3.0-or-later. Third-party licences are listed in
[`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md).

### Rights holders
If you believe something in this project infringes your rights, please open an issue or contact the
maintainer through GitHub. We will look at it promptly and in good faith.

---

## 日本語

**Washi は個人による非公式のオープンソースプロジェクトです。** キヤノン株式会社およびそのグループ
会社とは一切関係がなく、承認・後援・サポートを受けているものではありません。

### 商標について
Canon、キヤノン、PIXUS、PIXMA、Canon PRINT、およびキヤノン製用紙の製品名は、キヤノン株式会社の
商標または登録商標です。Washi では、対応するプリンターや用紙を示す目的でのみこれらの名称を使用して
います。その他の商標はそれぞれの権利者に帰属します。

### このリポジトリに含まれるもの・含まれないもの
- ソースコード、画面デザイン、アイコン、イラスト、文章、日本語訳はすべて独自に作成したものです。
- キヤノンのソフトウェア、ファームウェア、ソースコード、逆コンパイルしたコード、画像、アイコン、
  フォント、スクリーンショット、画面の文言は**一切含まれていません**。リリースのたびに、メーカーの
  文字列テーブルおよびファイルとの照合チェックを行っています。
- プリンターとの通信には、オープンな標準規格（印刷は IPP〔PWG〕、スキャンは eSCL〔Mopria〕）と、
  プリンター本体が LAN 上で提供している管理用インターフェース（CHMP/IVEC）を使用しており、
  いずれも独自に実装しています。

### プロトコル情報の取得方法について
管理用プロトコルの情報は、作者自身が所有するプリンターの動作を観察し、作者自身のパソコンに正規に
インストールしたソフトウェアを解析することで、独立したプログラムをそのプリンターと連携させる
（相互運用性を確保する）目的に限って取得しました。日本では、このようなプログラムの調査解析は
著作権法第30条の4の「著作物に表現された思想又は感情の享受を目的としない利用」に当たると
されています（文化庁「デジタル化・ネットワーク化の進展に対応した柔軟な権利制限規定に関する
基本的な考え方」令和元年10月、問12）。公開しているのは相互運用に必要な事実情報（コマンド名、
要素名、値の範囲など）のみで、メーカーのコードは含みません。その内容は
[`docs/PROTOCOL.md`](docs/PROTOCOL.md) にまとめています。

### Washi が行わないこと
- アクセス制御、コピープロテクト、認証の回避は一切行いません。
- インクカートリッジの認証やサービス用カウンターの読み出し・改変・回避は行わず、プリンターの
  ファームウェアイメージを読み出したり、改変したり、置き換えたりすることもありません。任意の
  ファームウェア更新機能は、プリンター自身に更新を実行するよう依頼するだけで（更新データは
  プリンターが自分で取得します）、実機では未検証であり、実行前に必ず確認を求めます。
- メーカーのネットワーク設定用の認証情報は含みません。新しいプリンターの Wi-Fi 設定は、
  プリンター本体の操作パネルまたは WPS で行ってください。
- 利用状況などの情報送信（テレメトリー）は行わず、指定したプリンター以外には接続しません
  （プリンターのマニュアルなど、Washi が確認したうえでユーザーが自分で開く Web ページを除きます）。

### ご利用にあたって
Washi は**無保証**で提供されます（ライセンスをご確認ください）。メンテナンス機能はインクや用紙を
消費します。画面の案内に従って操作してください。ファームウェア更新中は絶対にプリンターの電源を
切らないでください。

### ライセンス
GPL-2.0-or-later（`LICENSE` を参照）。一部の Rust 依存ライブラリが Apache-2.0 のみで提供されて
おり、これは GPL バージョン3と互換であるため、配布するビルド済みバイナリは GPL-3.0-or-later の
条件で提供されます。第三者のライセンスは [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md) に
記載しています。

### 権利者の方へ
本プロジェクトの内容が権利を侵害しているとお考えの場合は、GitHub の Issue またはメンテナーへの
連絡でお知らせください。速やかに誠実に対応いたします。
