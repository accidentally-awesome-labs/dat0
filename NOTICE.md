# NOTICE

```
dat0
Copyright 2026 Accidentally Awesome Labs

This product includes software developed by Accidentally Awesome Labs
and contributors.

Licensed under the Apache License, Version 2.0 (the "License").
You may obtain a copy of the License at:

    http://www.apache.org/licenses/LICENSE-2.0
```

## Third-party software

dat0 incorporates the following third-party components. The list below is generated mechanically by [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) against the actual workspace dependency tree, and a CI gate (`.github/workflows/notice.yml`) fails any PR that drifts from the regenerated output. Do not edit between the marker comments by hand — re-run `cargo about generate -c about.toml docs/about-template.hbs` and replace the marked block.

## Bundled assets

dat0 embeds Lucide icons and the Geist / Geist Mono typefaces in the
application binary. `cargo-about` sees only the crates dat0 depends on, not the
artwork or fonts inside or beside them, so both are recorded here by hand.

### Icons

- **14 icons** are vendored directly into `crates/dat0-ui/assets/icons/`:
  `funnel.svg`, `play.svg`, `layers.svg`, `bookmark.svg`, `clock.svg`,
  `database.svg`, `plug.svg`, `sparkles.svg`, and — added by the Dioxus
  migration, which no longer has a widget library supplying them —
  `close.svg`, `chevron-down.svg`, `chevron-up.svg`, `chevron-right.svg`,
  `chevrons-up-down.svg`, `search.svg`.
- **86 further icons** still reach the GPUI build through the
  `gpui-component-assets` crate (listed in the generated section below as an
  Apache-2.0 dependency; the artwork inside it is Lucide's). That crate leaves
  the tree when `crates/dat0-app` does, at which point the vendored set above
  is the whole inventory.

Lucide is dual-licensed. Most icons are ISC; icons derived from the Feather
project are MIT (Copyright (c) 2013-present Cole Bemis). dat0 ships icons under
both — `clock`, `database`, `x`/`close`, `search` and the `chevron-*` family are
among the Feather-derived set (`plug` and `sparkles` are not: they do not appear
in the authoritative list below). The complete upstream license text covering
both, including the authoritative list of Feather-derived icons, is vendored
verbatim at `crates/dat0-ui/assets/icons/LICENSE-lucide`.

```
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Fonts

dat0 embeds eight TrueType faces vendored into `crates/dat0-ui/assets/fonts/`
and registered at boot by `dat0_app::assets::register_fonts`:

- **Geist** — `Geist-Regular.ttf`, `Geist-Medium.ttf`, `Geist-SemiBold.ttf`,
  `Geist-Bold.ttf`
- **Geist Mono** — `GeistMono-Regular.ttf`, `GeistMono-Medium.ttf`,
  `GeistMono-SemiBold.ttf`, `GeistMono-Bold.ttf`

Both families come from <https://github.com/vercel/geist-font> (`fonts/Geist/ttf/`
and `fonts/GeistMono/ttf/`) and are licensed under the **SIL Open Font License,
Version 1.1** (SPDX: `OFL-1.1`). Fonts are not Cargo dependencies, so
`cargo-about` cannot see them; this section is maintained by hand alongside the
icon section above.

The complete upstream license text is vendored verbatim at
`crates/dat0-ui/assets/fonts/LICENSE-geist`. Its copyright line declares **no
Reserved Font Name**, so OFL 1.1 §3 imposes no rename obligation on
redistribution; dat0 ships the faces unmodified in any case.

```
Copyright 2024 The Geist Project Authors (https://github.com/vercel/geist-font)

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
https://openfontlicense.org
```

OFL 1.1 §2 requires that this notice travel with the fonts, which is why the
full text is vendored next to them rather than only summarised here.

### Vendored JavaScript

dat0's SQL console embeds a prebuilt CodeMirror 6 bundle at
`crates/dat0-ui/assets/codemirror.js`, served out of the binary over the `dat0`
custom asset protocol. `cargo-about` reads `Cargo.lock` and cannot see vendored
JavaScript, so this entry is maintained by hand. The build inputs — including
the exact pinned versions and `package-lock.json` — live in
`crates/dat0-ui/vendor/codemirror/`.

The bundle contains `@codemirror/state`, `@codemirror/view`,
`@codemirror/commands`, `@codemirror/lang-sql`, `@codemirror/autocomplete`,
`@codemirror/search`, `@codemirror/language` and `@lezer/highlight`. All are MIT.

```
MIT License

Copyright (C) 2018-2021 by Marijn Haverbeke <marijn@haverbeke.berlin> and others

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

<!-- BEGIN cargo-about generated -->
## BSD Zero Clause License (SPDX: 0BSD)

Used by:
- doctest-file 1.1.1 — https://codeberg.org/Goat7658/doctest-file

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- sentry-backtrace 0.36.0 — https://github.com/getsentry/sentry-rust
- sentry-core 0.36.0 — https://github.com/getsentry/sentry-rust
- sentry-panic 0.36.0 — https://github.com/getsentry/sentry-rust
- sentry-types 0.36.0 — https://github.com/getsentry/sentry-rust
- sentry 0.36.0 — https://github.com/getsentry/sentry-rust

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- xkeysym 0.2.1 — https://github.com/notgull/xkeysym

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- powerfmt 0.2.0 — https://github.com/jhpratt/powerfmt

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- deranged 0.5.8 — https://github.com/jhpratt/deranged

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- arrow-arith 56.2.0 — https://github.com/apache/arrow-rs
- arrow-array 56.2.0 — https://github.com/apache/arrow-rs
- arrow-buffer 56.2.0 — https://github.com/apache/arrow-rs
- arrow-cast 56.2.0 — https://github.com/apache/arrow-rs
- arrow-data 56.2.0 — https://github.com/apache/arrow-rs
- arrow-ord 56.2.0 — https://github.com/apache/arrow-rs
- arrow-row 56.2.0 — https://github.com/apache/arrow-rs
- arrow-schema 56.2.0 — https://github.com/apache/arrow-rs
- arrow-select 56.2.0 — https://github.com/apache/arrow-rs
- arrow-string 56.2.0 — https://github.com/apache/arrow-rs
- arrow 56.2.0 — https://github.com/apache/arrow-rs
- iri-string 0.7.12 — https://github.com/lo48576/iri-string
- utf8_iter 1.0.4 — https://github.com/hsivonen/utf8_iter
- x11rb-protocol 0.13.2 — https://github.com/psychon/x11rb
- x11rb 0.13.2 — https://github.com/psychon/x11rb
- zeroize 1.8.2 — https://github.com/RustCrypto/utils

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- zerocopy-derive 0.8.48 — https://github.com/google/zerocopy
- zerocopy 0.8.48 — https://github.com/google/zerocopy

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- moxcms 0.8.1 — https://github.com/awxkee/moxcms.git
- pxfm 0.1.29 — https://github.com/awxkee/pxfm

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- imgref 1.12.0 — https://github.com/kornelski/imgref
- ureq 2.12.1 — https://github.com/algesten/ureq
- zune-core 0.5.1 — https://github.com/etemesi254/zune-image
- zune-jpeg 0.5.15 — https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- ipnet 2.12.0 — https://github.com/krisprice/ipnet

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- bit_field 0.10.3 — https://github.com/phil-opp/rust-bit-field

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- anstream 1.0.0 — https://github.com/rust-cli/anstyle.git
- anstyle-parse 1.0.0 — https://github.com/rust-cli/anstyle.git
- anstyle-query 1.1.5 — https://github.com/rust-cli/anstyle.git
- anstyle 1.0.14 — https://github.com/rust-cli/anstyle.git
- clap 4.6.1 — https://github.com/clap-rs/clap
- clap_builder 4.6.0 — https://github.com/clap-rs/clap
- clap_derive 4.6.1 — https://github.com/clap-rs/clap
- clap_lex 1.1.0 — https://github.com/clap-rs/clap
- colorchoice 1.0.5 — https://github.com/rust-cli/anstyle.git
- crc32fast 1.5.0 — https://github.com/srijs/rust-crc32fast
- fallible-iterator 0.3.0 — https://github.com/sfackler/rust-fallible-iterator
- fallible-streaming-iterator 0.1.9 — https://github.com/sfackler/fallible-streaming-iterator
- float-ord 0.3.2 — https://github.com/notriddle/rust-float-ord
- foreign-types-macros 0.2.3 — https://github.com/sfackler/foreign-types
- foreign-types-shared 0.1.1 — https://github.com/sfackler/foreign-types
- foreign-types-shared 0.3.1 — https://github.com/sfackler/foreign-types
- foreign-types 0.3.2 — https://github.com/sfackler/foreign-types
- foreign-types 0.5.0 — https://github.com/sfackler/foreign-types
- hex 0.4.3 — https://github.com/KokaKiwi/rust-hex
- is_terminal_polyfill 1.70.2 — https://github.com/polyfill-rs/is_terminal_polyfill
- native-tls 0.2.18 — https://github.com/rust-native-tls/rust-native-tls
- no_std_io2 0.9.3 — https://github.com/wcampbell0x2a/no-std-io2
- openssl-macros 0.1.1
- openssl 0.10.81 — https://github.com/rust-openssl/rust-openssl
- quick-error 2.0.1 — http://github.com/tailhook/quick-error
- serde_spanned 0.6.9 — https://github.com/toml-rs/toml
- toml 0.8.2 — https://github.com/toml-rs/toml
- toml_datetime 1.1.1+spec-1.1.0 — https://github.com/toml-rs/toml
- toml_edit 0.19.15 — https://github.com/toml-rs/toml
- toml_edit 0.20.2 — https://github.com/toml-rs/toml
- toml_edit 0.25.11+spec-1.1.0 — https://github.com/toml-rs/toml
- toml_parser 1.1.2+spec-1.1.0 — https://github.com/toml-rs/toml

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- async-broadcast 0.7.2 — https://github.com/smol-rs/async-broadcast

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- futures-channel 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-core 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-executor 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-io 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-macro 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-sink 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-task 0.3.32 — https://github.com/rust-lang/futures-rs
- futures-util 0.3.32 — https://github.com/rust-lang/futures-rs
- futures 0.3.32 — https://github.com/rust-lang/futures-rs

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- typenum 1.20.0 — https://github.com/paholg/typenum

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- reqwest 0.12.28 — https://github.com/seanmonstar/reqwest

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- cookie 0.18.2 — https://github.com/SergioBenitez/cookie-rs

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- http 1.4.0 — https://github.com/hyperium/http

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- tokio-rustls 0.26.4 — https://github.com/rustls/tokio-rustls

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- ppv-lite86 0.2.21 — https://github.com/cryptocorrosion/cryptocorrosion

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- proc-macro-error-attr 1.0.4 — https://gitlab.com/CreepySkeleton/proc-macro-error

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- iana-time-zone 0.1.65 — https://github.com/strawlab/iana-time-zone

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- rustls-pki-types 1.14.1 — https://github.com/rustls/pki-types

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- memmap2 0.9.10 — https://github.com/RazrFalcon/memmap2-rs

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- async-recursion 1.1.1 — https://github.com/dcchut/async-recursion
- gif 0.14.2 — https://github.com/image-rs/image-gif
- keyboard-types 0.7.0 — https://github.com/pyfisch/keyboard-types
- weezl 0.1.12 — https://github.com/image-rs/weezl

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- addr2line 0.25.1 — https://github.com/gimli-rs/addr2line
- ahash 0.8.12 — https://github.com/tkaitchuck/ahash
- aligned 0.4.3 — https://github.com/rust-embedded-community/aligned
- arrayvec 0.7.6 — https://github.com/bluss/arrayvec
- as-slice 0.2.1 — https://github.com/japaric/as-slice
- async-channel 2.5.0 — https://github.com/smol-rs/async-channel
- async-executor 1.14.0 — https://github.com/smol-rs/async-executor
- async-fs 2.2.0 — https://github.com/smol-rs/async-fs
- async-io 2.6.0 — https://github.com/smol-rs/async-io
- async-lock 3.4.2 — https://github.com/smol-rs/async-lock
- async-net 2.0.0 — https://github.com/smol-rs/async-net
- async-process 2.5.0 — https://github.com/smol-rs/async-process
- async-signal 0.2.14 — https://github.com/smol-rs/async-signal
- async-task 4.7.1 — https://github.com/smol-rs/async-task
- atomic-waker 1.1.2 — https://github.com/smol-rs/atomic-waker
- backtrace 0.3.76 — https://github.com/rust-lang/backtrace-rs
- base64 0.22.1 — https://github.com/marshallpierce/rust-base64
- bitflags 1.3.2 — https://github.com/bitflags/bitflags
- bitflags 2.11.1 — https://github.com/bitflags/bitflags
- bitstream-io 4.10.0 — https://github.com/tuffy/bitstream-io
- blocking 1.6.2 — https://github.com/smol-rs/blocking
- bstr 1.12.1 — https://github.com/BurntSushi/bstr
- cast 0.3.0 — https://github.com/japaric/cast.rs
- cfg-if 1.0.4 — https://github.com/rust-lang/cfg-if
- cocoa-foundation 0.2.0 — https://github.com/servo/core-foundation-rs
- cocoa 0.26.1 — https://github.com/servo/core-foundation-rs
- concurrent-queue 2.5.0 — https://github.com/smol-rs/concurrent-queue
- core-foundation-sys 0.8.7 — https://github.com/servo/core-foundation-rs
- core-foundation 0.10.1 — https://github.com/servo/core-foundation-rs
- core-foundation 0.9.4 — https://github.com/servo/core-foundation-rs
- core-graphics-types 0.1.3 — https://github.com/servo/core-foundation-rs
- core-graphics-types 0.2.0 — https://github.com/servo/core-foundation-rs
- core-graphics 0.23.2 — https://github.com/servo/core-foundation-rs
- core-graphics 0.24.0 — https://github.com/servo/core-foundation-rs
- core-graphics 0.25.0 — https://github.com/servo/core-foundation-rs
- core-text 20.1.0 — https://github.com/servo/core-foundation-rs
- crossbeam-channel 0.5.15 — https://github.com/crossbeam-rs/crossbeam
- crossbeam-deque 0.8.6 — https://github.com/crossbeam-rs/crossbeam
- crossbeam-epoch 0.9.20 — https://github.com/crossbeam-rs/crossbeam
- crossbeam-utils 0.8.21 — https://github.com/crossbeam-rs/crossbeam
- debugid 0.8.0 — https://github.com/getsentry/rust-debugid
- displaydoc 0.2.5 — https://github.com/yaahc/displaydoc
- either 1.15.0 — https://github.com/rayon-rs/either
- enumset 1.1.14 — https://github.com/Lymia/enumset
- enumset_derive 0.15.0 — https://github.com/Lymia/enumset
- equivalent 1.0.2 — https://github.com/indexmap-rs/equivalent
- errno 0.3.14 — https://github.com/lambda-fairy/rust-errno
- euclid 0.22.14 — https://github.com/servo/euclid
- event-listener-strategy 0.5.4 — https://github.com/smol-rs/event-listener-strategy
- event-listener 5.4.1 — https://github.com/smol-rs/event-listener
- fastrand 2.4.1 — https://github.com/smol-rs/fastrand
- filetime 0.2.27 — https://github.com/alexcrichton/filetime
- flate2 1.1.9 — https://github.com/rust-lang/flate2-rs
- fnv 1.0.7 — https://github.com/servo/rust-fnv
- font-kit 0.14.3 — https://github.com/servo/font-kit
- form_urlencoded 1.2.2 — https://github.com/servo/rust-url
- fs4 0.9.1 — https://github.com/al8n/fs4-rs
- futures-lite 2.6.1 — https://github.com/smol-rs/futures-lite
- gethostname 1.1.0 — https://codeberg.org/swsnr/gethostname.rs.git
- gimli 0.32.3 — https://github.com/gimli-rs/gimli
- global-hotkey 0.7.0 — https://github.com/amrbashir/global-hotkey
- hashbrown 0.14.5 — https://github.com/rust-lang/hashbrown
- hashbrown 0.15.5 — https://github.com/rust-lang/hashbrown
- hashbrown 0.16.1 — https://github.com/rust-lang/hashbrown
- hashbrown 0.17.0 — https://github.com/rust-lang/hashbrown
- heck 0.4.1 — https://github.com/withoutboats/heck
- heck 0.5.0 — https://github.com/withoutboats/heck
- httparse 1.10.1 — https://github.com/seanmonstar/httparse
- hyper-rustls 0.27.9 — https://github.com/rustls/hyper-rustls
- idna 1.1.0 — https://github.com/servo/rust-url/
- idna_adapter 1.2.1 — https://github.com/hsivonen/idna_adapter
- indexmap 2.14.0 — https://github.com/indexmap-rs/indexmap
- itertools 0.14.0 — https://github.com/rust-itertools/itertools
- jpeg-decoder 0.3.2 — https://github.com/image-rs/jpeg-decoder
- lazy_static 1.5.0 — https://github.com/rust-lang-nursery/lazy-static.rs
- libappindicator 0.9.0
- linux-raw-sys 0.12.1 — https://github.com/sunfishcode/linux-raw-sys
- linux-raw-sys 0.4.15 — https://github.com/sunfishcode/linux-raw-sys
- lock_api 0.4.14 — https://github.com/Amanieu/parking_lot
- log 0.4.29 — https://github.com/rust-lang/log
- longest-increasing-subsequence 0.1.0 — https://github.com/fitzgen/longest-increasing-subsequence
- muda 0.17.2 — https://github.com/tauri-apps/muda
- num-bigint 0.4.6 — https://github.com/rust-num/num-bigint
- num-complex 0.4.6 — https://github.com/rust-num/num-complex
- num-derive 0.4.2 — https://github.com/rust-num/num-derive
- num-integer 0.1.46 — https://github.com/rust-num/num-integer
- num-iter 0.1.45 — https://github.com/rust-num/num-iter
- num-rational 0.4.2 — https://github.com/rust-num/num-rational
- num-traits 0.2.19 — https://github.com/rust-num/num-traits
- num 0.4.3 — https://github.com/rust-num/num
- num_cpus 1.17.0 — https://github.com/seanmonstar/num_cpus
- object 0.37.3 — https://github.com/gimli-rs/object
- once_cell 1.21.4 — https://github.com/matklad/once_cell
- openssl-probe 0.2.1 — https://github.com/rustls/openssl-probe
- ordered-stream 0.2.0 — https://github.com/danieldg/ordered-stream
- parking 2.2.1 — https://github.com/smol-rs/parking
- parking_lot 0.12.5 — https://github.com/Amanieu/parking_lot
- parking_lot_core 0.9.12 — https://github.com/Amanieu/parking_lot
- percent-encoding 2.3.2 — https://github.com/servo/rust-url/
- piper 0.2.5 — https://github.com/smol-rs/piper
- png 0.17.16 — https://github.com/image-rs/image-png
- png 0.18.1 — https://github.com/image-rs/image-png
- polling 3.11.0 — https://github.com/smol-rs/polling
- pollster 0.4.0 — https://github.com/zesterer/pollster
- rayon-core 1.13.0 — https://github.com/rayon-rs/rayon
- rayon 1.12.0 — https://github.com/rayon-rs/rayon
- regex-automata 0.4.14 — https://github.com/rust-lang/regex
- regex-syntax 0.8.10 — https://github.com/rust-lang/regex
- regex 1.12.3 — https://github.com/rust-lang/regex
- ring 0.17.14 — https://github.com/briansmith/ring
- rustc-demangle 0.1.27 — https://github.com/rust-lang/rustc-demangle
- rustc-hash 1.1.0 — https://github.com/rust-lang-nursery/rustc-hash
- rustix 0.38.44 — https://github.com/bytecodealliance/rustix
- rustix 1.1.4 — https://github.com/bytecodealliance/rustix
- rustls 0.23.39 — https://github.com/rustls/rustls
- scoped-tls 1.0.1 — https://github.com/alexcrichton/scoped-tls
- scopeguard 1.2.0 — https://github.com/bluss/scopeguard
- secret-service 5.1.0 — https://github.com/hwchen/secret-service-rs.git
- security-framework-sys 2.17.0 — https://github.com/kornelski/rust-security-framework
- security-framework 2.11.1 — https://github.com/kornelski/rust-security-framework
- security-framework 3.7.0 — https://github.com/kornelski/rust-security-framework
- shellexpand 3.1.2 — https://gitlab.com/ijackson/rust-shellexpand
- signal-hook-registry 1.4.8 — https://github.com/vorner/signal-hook
- signal-hook 0.3.18 — https://github.com/vorner/signal-hook
- smallvec 1.15.1 — https://github.com/servo/rust-smallvec
- socket2 0.6.3 — https://github.com/rust-lang/socket2
- stable_deref_trait 1.2.1 — https://github.com/storyyeller/stable_deref_trait
- syn 1.0.109 — https://github.com/dtolnay/syn
- tempfile 3.27.0 — https://github.com/Stebalien/tempfile
- thread_local 1.1.9 — https://github.com/Amanieu/thread_local-rs
- toml_datetime 0.6.3 — https://github.com/toml-rs/toml
- tray-icon 0.21.3 — https://github.com/tauri-apps/tray-icon
- ttf-parser 0.20.0 — https://github.com/RazrFalcon/ttf-parser
- tungstenite 0.28.0 — https://github.com/snapview/tungstenite-rs
- unicode-segmentation 1.13.2 — https://github.com/unicode-rs/unicode-segmentation
- unicode-width 0.2.2 — https://github.com/unicode-rs/unicode-width
- unicode-xid 0.2.6 — https://github.com/unicode-rs/unicode-xid
- url 2.5.8 — https://github.com/servo/rust-url
- uuid 1.23.1 — https://github.com/uuid-rs/uuid
- wry 0.53.5 — https://github.com/tauri-apps/wry

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- hashlink 0.10.0 — https://github.com/kyren/hashlink
- hashlink 0.9.1 — https://github.com/kyren/hashlink

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- downcast-rs 1.2.1 — https://github.com/marcianx/downcast-rs
- lexical-core 1.0.6 — https://github.com/Alexhuszagh/rust-lexical
- lexical-parse-float 1.0.6 — https://github.com/Alexhuszagh/rust-lexical
- lexical-parse-integer 1.0.6 — https://github.com/Alexhuszagh/rust-lexical
- lexical-util 1.0.7 — https://github.com/Alexhuszagh/rust-lexical
- lexical-write-float 1.0.6 — https://github.com/Alexhuszagh/rust-lexical
- lexical-write-integer 1.0.6 — https://github.com/Alexhuszagh/rust-lexical
- qoi 0.4.1 — https://github.com/aldanor/qoi-rust

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- hkdf 0.12.4 — https://github.com/RustCrypto/KDFs/

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- aes 0.8.4 — https://github.com/RustCrypto/block-ciphers
- block-buffer 0.10.4 — https://github.com/RustCrypto/utils
- block-padding 0.3.3 — https://github.com/RustCrypto/utils
- cbc 0.1.2 — https://github.com/RustCrypto/block-modes
- cipher 0.4.4 — https://github.com/RustCrypto/traits
- cpufeatures 0.2.17 — https://github.com/RustCrypto/utils
- crypto-common 0.1.7 — https://github.com/RustCrypto/traits
- digest 0.10.7 — https://github.com/RustCrypto/traits
- hmac 0.12.1 — https://github.com/RustCrypto/MACs
- inout 0.1.4 — https://github.com/RustCrypto/utils
- sha1 0.10.7 — https://github.com/RustCrypto/hashes
- sha2 0.10.9 — https://github.com/RustCrypto/hashes

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- webbrowser 1.2.4 — https://github.com/amodm/webbrowser-rs

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- rand_core 0.6.4 — https://github.com/rust-random/rand
- rand_core 0.9.5 — https://github.com/rust-random/rand

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- getrandom 0.2.17 — https://github.com/rust-random/getrandom
- getrandom 0.3.4 — https://github.com/rust-random/getrandom
- getrandom 0.4.2 — https://github.com/rust-random/getrandom
- rand_chacha 0.3.1 — https://github.com/rust-random/rand

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- adler2 2.0.1 — https://github.com/oyvindln/adler2
- proc-macro-crate 1.3.1 — https://github.com/bkchr/proc-macro-crate
- proc-macro-crate 2.0.2 — https://github.com/bkchr/proc-macro-crate
- proc-macro-crate 3.5.0 — https://github.com/bkchr/proc-macro-crate

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- proc-macro-error 1.0.4 — https://gitlab.com/CreepySkeleton/proc-macro-error

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- enumflags2 0.7.12 — https://github.com/meithecatte/enumflags2

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- enumflags2_derive 0.7.12 — https://github.com/meithecatte/enumflags2

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- dpi 0.1.2 — https://github.com/rust-windowing/winit
- tao 0.34.8 — https://github.com/tauri-apps/tao

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- bytemuck 1.25.0 — https://github.com/Lokathor/bytemuck

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- httpdate 1.0.3 — https://github.com/pyfisch/httpdate

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- dat0-core 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-engine 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-fixtures 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-format 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-i18n 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-keychain 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- dat0-ui 0.1.0 — https://github.com/accidentally-awesome-labs/dat0
- xtask 0.1.0
- allocator-api2 0.2.21 — https://github.com/zakarumych/allocator-api2
- anyhow 1.0.104 — https://github.com/dtolnay/anyhow
- arboard 3.6.1 — https://github.com/1Password/arboard
- async-trait 0.1.89 — https://github.com/dtolnay/async-trait
- const-serialize-macro 0.7.2 — https://github.com/dioxuslabs/dioxus
- const-serialize-macro 0.8.0-alpha.1 — https://github.com/dioxuslabs/dioxus
- const-serialize 0.7.2 — https://github.com/dioxuslabs/dioxus
- const-serialize 0.8.0-alpha.0 — https://github.com/dioxuslabs/dioxus
- dioxus-asset-resolver 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-cli-config 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-config-macro 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-config-macros 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-core-macro 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-core-types 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-core 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-desktop 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-devtools-types 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-devtools 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-document 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-history 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-hooks 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-html-internal-macro 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-html 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-interpreter-js 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-rsx 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-signals 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-stores-macro 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus-stores 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dioxus 0.7.10 — https://github.com/DioxusLabs/dioxus/
- dirs-sys 0.4.1 — https://github.com/dirs-dev/dirs-sys-rs
- dirs-sys 0.5.0 — https://github.com/dirs-dev/dirs-sys-rs
- dirs 5.0.1 — https://github.com/soc/dirs-rs
- dirs 6.0.0 — https://github.com/soc/dirs-rs
- dispatch2 0.3.1 — https://github.com/madsmtm/objc2
- dunce 1.0.5 — https://gitlab.com/kornelski/dunce
- fdeflate 0.3.7 — https://github.com/image-rs/fdeflate
- field-offset 0.3.6 — https://github.com/Diggsey/rust-field-offset
- generational-box 0.7.10 — https://github.com/DioxusLabs/dioxus/
- half 2.7.1 — https://github.com/VoidStarKat/half-rs
- ident_case 1.0.1 — https://github.com/TedDriggs/ident_case
- image-webp 0.2.4 — https://github.com/image-rs/image-webp
- image 0.24.9 — https://github.com/image-rs/image
- image 0.25.10 — https://github.com/image-rs/image
- interprocess 2.4.2 — https://github.com/kotauskas/interprocess
- itoa 1.0.18 — https://github.com/dtolnay/itoa
- libappindicator-sys 0.9.0
- libc 0.2.186 — https://github.com/rust-lang/libc
- macro-string 0.1.4 — https://github.com/dtolnay/macro-string
- manganis-core 0.7.10 — https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis-core
- manganis-macro 0.7.10 — https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis-macro
- manganis 0.7.10 — https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis
- miniz_oxide 0.8.9 — https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide
- num-conv 0.2.1 — https://github.com/jhpratt/num-conv
- objc2-app-kit 0.3.2 — https://github.com/madsmtm/objc2
- objc2-core-foundation 0.3.2 — https://github.com/madsmtm/objc2
- objc2-core-graphics 0.3.2 — https://github.com/madsmtm/objc2
- objc2-exception-helper 0.1.1 — https://github.com/madsmtm/objc2
- objc2-web-kit 0.3.2 — https://github.com/madsmtm/objc2
- paste 1.0.15 — https://github.com/dtolnay/paste
- pastey 0.1.1 — https://github.com/as1100k/pastey
- pathfinder_geometry 0.5.1 — https://github.com/servo/pathfinder
- pathfinder_simd 0.5.6 — https://github.com/servo/pathfinder
- pin-project-internal 1.1.11 — https://github.com/taiki-e/pin-project
- pin-project-lite 0.2.17 — https://github.com/taiki-e/pin-project-lite
- pin-project 1.1.11 — https://github.com/taiki-e/pin-project
- proc-macro2-diagnostics 0.10.1 — https://github.com/SergioBenitez/proc-macro2-diagnostics
- proc-macro2 1.0.106 — https://github.com/dtolnay/proc-macro2
- profiling-procmacros 1.0.17 — https://github.com/aclysma/profiling
- profiling 1.0.17 — https://github.com/aclysma/profiling
- quote 1.0.45 — https://github.com/dtolnay/quote
- rand 0.8.6 — https://github.com/rust-random/rand
- rand 0.9.4 — https://github.com/rust-random/rand
- rand_chacha 0.9.0 — https://github.com/rust-random/rand
- raw-window-handle 0.5.2 — https://github.com/rust-windowing/raw-window-handle
- raw-window-handle 0.6.2 — https://github.com/rust-windowing/raw-window-handle
- rustc-hash 2.1.2 — https://github.com/rust-lang/rustc-hash
- rustversion 1.0.22 — https://github.com/dtolnay/rustversion
- ryu 1.0.23 — https://github.com/dtolnay/ryu
- serde 1.0.228 — https://github.com/serde-rs/serde
- serde_core 1.0.228 — https://github.com/serde-rs/serde
- serde_derive 1.0.228 — https://github.com/serde-rs/serde
- serde_json 1.0.149 — https://github.com/serde-rs/json
- serde_repr 0.1.20 — https://github.com/dtolnay/serde-repr
- serde_urlencoded 0.7.1 — https://github.com/nox/serde_urlencoded
- subsecond-types 0.7.10 — https://github.com/DioxusLabs/dioxus/tree/main/packages/subsecond
- subsecond 0.7.10 — https://github.com/DioxusLabs/dioxus/tree/main/packages/subsecond
- syn 2.0.117 — https://github.com/dtolnay/syn
- sync_wrapper 1.0.2 — https://github.com/Actyx/sync_wrapper
- thiserror-impl 1.0.69 — https://github.com/dtolnay/thiserror
- thiserror-impl 2.0.18 — https://github.com/dtolnay/thiserror
- thiserror 1.0.69 — https://github.com/dtolnay/thiserror
- thiserror 2.0.18 — https://github.com/dtolnay/thiserror
- time-core 0.1.8 — https://github.com/time-rs/time
- time-macros 0.2.27 — https://github.com/time-rs/time
- time 0.3.47 — https://github.com/time-rs/time
- typed-path 0.12.3 — https://github.com/chipsenkbeil/typed-path
- unicode-ident 1.0.24 — https://github.com/dtolnay/unicode-ident
- utf-8 0.7.6 — https://github.com/SimonSapin/rust-utf8
- utf8parse 0.2.2 — https://github.com/alacritty/vte
- warnings-macro 0.2.0 — https://github.com/dioxuslabs/warnings
- warnings 0.2.1 — https://github.com/dioxuslabs/warnings
- zune-inflate 0.2.54

## Apache License 2.0 (SPDX: Apache-2.0)

Used by:
- chrono 0.4.44 — https://github.com/chronotope/chrono

## BSD 2-Clause &quot;Simplified&quot; License (SPDX: BSD-2-Clause)

Used by:
- v_frame 0.3.9 — https://github.com/rust-av/v_frame

## BSD 2-Clause &quot;Simplified&quot; License (SPDX: BSD-2-Clause)

Used by:
- rav1e 0.8.1 — https://github.com/xiph/rav1e/

## BSD 2-Clause &quot;Simplified&quot; License (SPDX: BSD-2-Clause)

Used by:
- av1-grain 0.2.5 — https://github.com/rust-av/av1-grain

## BSD 3-Clause &quot;New&quot; or &quot;Revised&quot; License (SPDX: BSD-3-Clause)

Used by:
- avif-serialize 0.8.8 — https://github.com/kornelski/avif-serialize

## BSD 3-Clause &quot;New&quot; or &quot;Revised&quot; License (SPDX: BSD-3-Clause)

Used by:
- ravif 0.13.0 — https://github.com/kornelski/cavif-rs

## BSD 3-Clause &quot;New&quot; or &quot;Revised&quot; License (SPDX: BSD-3-Clause)

Used by:
- subtle 2.6.1 — https://github.com/dalek-cryptography/subtle

## BSD 3-Clause &quot;New&quot; or &quot;Revised&quot; License (SPDX: BSD-3-Clause)

Used by:
- lebe 0.5.3 — https://github.com/johannesvollmer/lebe

## BSD 3-Clause &quot;New&quot; or &quot;Revised&quot; License (SPDX: BSD-3-Clause)

Used by:
- exr 1.74.0 — https://github.com/johannesvollmer/exrs

## Creative Commons Zero v1.0 Universal (SPDX: CC0-1.0)

Used by:
- notify 6.1.1 — https://github.com/notify-rs/notify.git

## Community Data License Agreement Permissive 2.0 (SPDX: CDLA-Permissive-2.0)

Used by:
- webpki-roots 0.26.11 — https://github.com/rustls/webpki-roots
- webpki-roots 1.0.7 — https://github.com/rustls/webpki-roots

## ISC License (SPDX: ISC)

Used by:
- untrusted 0.9.0 — https://github.com/briansmith/untrusted

## ISC License (SPDX: ISC)

Used by:
- inotify-sys 0.1.5 — https://github.com/hannobraun/inotify-sys

## ISC License (SPDX: ISC)

Used by:
- inotify 0.9.6 — https://github.com/hannobraun/inotify

## ISC License (SPDX: ISC)

Used by:
- ring 0.17.14 — https://github.com/briansmith/ring

## ISC License (SPDX: ISC)

Used by:
- libloading 0.7.4 — https://github.com/nagisa/rust_libloading/
- libloading 0.8.9 — https://github.com/nagisa/rust_libloading/

## ISC License (SPDX: ISC)

Used by:
- rustls-webpki 0.103.13 — https://github.com/rustls/webpki

## MIT License (SPDX: MIT)

Used by:
- openssl-sys 0.9.117 — https://github.com/rust-openssl/rust-openssl

## MIT License (SPDX: MIT)

Used by:
- mio 0.8.11 — https://github.com/tokio-rs/mio
- mio 1.2.0 — https://github.com/tokio-rs/mio

## MIT License (SPDX: MIT)

Used by:
- nom 8.0.0 — https://github.com/rust-bakery/nom

## MIT License (SPDX: MIT)

Used by:
- libsqlite3-sys 0.28.0 — https://github.com/rusqlite/rusqlite
- rusqlite 0.31.0 — https://github.com/rusqlite/rusqlite

## MIT License (SPDX: MIT)

Used by:
- hyper 1.9.0 — https://github.com/hyperium/hyper

## MIT License (SPDX: MIT)

Used by:
- wayland-backend 0.3.15 — https://github.com/smithay/wayland-rs
- wayland-client 0.31.14 — https://github.com/smithay/wayland-rs
- wayland-protocols 0.32.12 — https://github.com/smithay/wayland-rs
- wayland-scanner 0.31.10 — https://github.com/smithay/wayland-rs
- wayland-sys 0.31.11 — https://github.com/smithay/wayland-rs

## MIT License (SPDX: MIT)

Used by:
- new_debug_unreachable 1.0.6 — https://github.com/mbrubeck/rust-debug-unreachable

## MIT License (SPDX: MIT)

Used by:
- dlib 0.5.3 — https://github.com/elinorbgr/dlib

## MIT License (SPDX: MIT)

Used by:
- webkit2gtk-sys 2.0.1 — https://github.com/tauri-apps/webkit2gtk-rs

## MIT License (SPDX: MIT)

Used by:
- webkit2gtk 2.0.1 — https://github.com/tauri-apps/webkit2gtk-rs

## MIT License (SPDX: MIT)

Used by:
- memoffset 0.9.1 — https://github.com/Gilnaa/memoffset

## MIT License (SPDX: MIT)

Used by:
- bytes 1.11.1 — https://github.com/tokio-rs/bytes

## MIT License (SPDX: MIT)

Used by:
- want 0.3.1 — https://github.com/seanmonstar/want

## MIT License (SPDX: MIT)

Used by:
- try-lock 0.2.5 — https://github.com/seanmonstar/try-lock

## MIT License (SPDX: MIT)

Used by:
- slab 0.4.12 — https://github.com/tokio-rs/slab

## MIT License (SPDX: MIT)

Used by:
- sharded-slab 0.1.7 — https://github.com/hawkw/sharded-slab

## MIT License (SPDX: MIT)

Used by:
- matchers 0.2.0 — https://github.com/hawkw/matchers

## MIT License (SPDX: MIT)

Used by:
- tracing-attributes 0.1.31 — https://github.com/tokio-rs/tracing
- tracing-core 0.1.36 — https://github.com/tokio-rs/tracing
- tracing-log 0.2.0 — https://github.com/tokio-rs/tracing
- tracing-subscriber 0.3.23 — https://github.com/tokio-rs/tracing
- tracing 0.1.44 — https://github.com/tokio-rs/tracing

## MIT License (SPDX: MIT)

Used by:
- tower-layer 0.3.3 — https://github.com/tower-rs/tower
- tower-service 0.3.3 — https://github.com/tower-rs/tower
- tower 0.5.3 — https://github.com/tower-rs/tower

## MIT License (SPDX: MIT)

Used by:
- tower-http 0.6.8 — https://github.com/tower-rs/tower-http

## MIT License (SPDX: MIT)

Used by:
- http-body 1.0.1 — https://github.com/hyperium/http-body

## MIT License (SPDX: MIT)

Used by:
- http-body-util 0.1.3 — https://github.com/hyperium/http-body

## MIT License (SPDX: MIT)

Used by:
- hyper-util 0.1.20 — https://github.com/hyperium/hyper-util

## MIT License (SPDX: MIT)

Used by:
- zbus 5.15.0 — https://github.com/z-galaxy/zbus/
- zbus_macros 5.15.0 — https://github.com/z-galaxy/zbus/
- zbus_names 4.3.2 — https://github.com/z-galaxy/zbus/
- zvariant 5.10.1 — https://github.com/z-galaxy/zbus/
- zvariant_derive 5.10.1 — https://github.com/z-galaxy/zbus/

## MIT License (SPDX: MIT)

Used by:
- synstructure 0.13.2 — https://github.com/mystor/synstructure

## MIT License (SPDX: MIT)

Used by:
- libduckdb-sys 1.4.4 — https://github.com/duckdb/duckdb-rs

## MIT License (SPDX: MIT)

Used by:
- lru 0.18.2 — https://github.com/jeromefroe/lru-rs.git

## MIT License (SPDX: MIT)

Used by:
- rust_decimal 1.41.0 — https://github.com/paupino/rust-decimal

## MIT License (SPDX: MIT)

Used by:
- atoi 2.0.0 — https://github.com/pacman82/atoi-rs

## MIT License (SPDX: MIT)

Used by:
- cfb 0.7.3 — https://github.com/mdsteele/rust-cfb

## MIT License (SPDX: MIT)

Used by:
- darling 0.21.3 — https://github.com/TedDriggs/darling
- darling_core 0.21.3 — https://github.com/TedDriggs/darling
- darling_macro 0.21.3 — https://github.com/TedDriggs/darling

## MIT License (SPDX: MIT)

Used by:
- arg_enum_proc_macro 0.3.4 — https://github.com/lu-zero/arg_enum_proc_macro

## MIT License (SPDX: MIT)

Used by:
- tiff 0.11.3 — https://github.com/image-rs/image-tiff

## MIT License (SPDX: MIT)

Used by:
- comfy-table 7.1.2 — https://github.com/nukesor/comfy-table

## MIT License (SPDX: MIT)

Used by:
- infer 0.19.0 — https://github.com/bojand/infer

## MIT License (SPDX: MIT)

Used by:
- rgb 0.8.53 — https://github.com/kornelski/rust-rgb

## MIT License (SPDX: MIT)

Used by:
- noop_proc_macro 0.3.0 — https://github.com/lu-zero/noop_proc_macro

## MIT License (SPDX: MIT)

Used by:
- av-scenechange 0.14.1 — https://github.com/rust-av/av-scenechange

## MIT License (SPDX: MIT)

Used by:
- strum 0.26.3 — https://github.com/Peternator7/strum
- strum 0.27.2 — https://github.com/Peternator7/strum
- strum_macros 0.26.4 — https://github.com/Peternator7/strum
- strum_macros 0.27.2 — https://github.com/Peternator7/strum

## MIT License (SPDX: MIT)

Used by:
- tokio-macros 2.7.0 — https://github.com/tokio-rs/tokio

## MIT License (SPDX: MIT)

Used by:
- ashpd 0.11.1 — https://github.com/bilelmoussaoui/ashpd
- rfd 0.17.2 — https://github.com/PolyMeilex/rfd

## MIT License (SPDX: MIT)

Used by:
- maybe-rayon 0.1.1 — https://github.com/shssoichiro/maybe-rayon

## MIT License (SPDX: MIT)

Used by:
- rfd 0.15.4 — https://github.com/PolyMeilex/rfd

## MIT License (SPDX: MIT)

Used by:
- aligned-vec 0.6.4 — https://github.com/sarah-ek/aligned-vec/

## MIT License (SPDX: MIT)

Used by:
- equator-macro 0.4.2 — https://github.com/sarah-ek/equator/
- equator 0.4.2 — https://github.com/sarah-ek/equator/

## MIT License (SPDX: MIT)

Used by:
- convert_case 0.8.0 — https://github.com/rutrum/convert-case

## MIT License (SPDX: MIT)

Used by:
- block2 0.6.2 — https://github.com/madsmtm/objc2
- block 0.1.6 — http://github.com/SSheldon/rust-block
- dioxus-logger 0.7.10 — https://github.com/dioxuslabs/dioxus
- dlopen2 0.8.2 — https://github.com/OpenByteDev/dlopen2
- dlopen2_derive 0.4.3 — https://github.com/OpenByteDev/dlopen2
- dpi 0.1.2 — https://github.com/rust-windowing/winit
- duckdb 1.4.4 — https://github.com/duckdb/duckdb-rs
- fax 0.2.6 — https://github.com/pdf-rs/fax
- fax_derive 0.2.0 — https://github.com/pdf-rs/fax
- libm 0.2.16 — https://github.com/rust-lang/compiler-builtins
- malloc_buf 0.0.6 — https://github.com/SSheldon/malloc_buf
- minisign-verify 0.2.5 — https://github.com/jedisct1/rust-minisign-verify
- objc2-encode 4.1.0 — https://github.com/madsmtm/objc2
- objc2-foundation 0.3.2 — https://github.com/madsmtm/objc2
- objc2 0.6.4 — https://github.com/madsmtm/objc2
- plotters-backend 0.3.7 — https://github.com/plotters-rs/plotters
- plotters-bitmap 0.3.7 — https://github.com/plotters-rs/plotters
- plotters-svg 0.3.7 — https://github.com/plotters-rs/plotters.git
- plotters 0.3.7 — https://github.com/plotters-rs/plotters
- simd_helpers 0.1.0 — https://github.com/lu-zero/simd_helpers
- sledgehammer_bindgen 0.6.0 — https://github.com/demonthos/sledgehammer_bindgen/
- sledgehammer_bindgen_macro 0.6.5 — https://github.com/demonthos/sledgehammer_bindgen/
- sledgehammer_utils 0.3.1 — https://github.com/demonthos/sledgehammer_utils/

## MIT License (SPDX: MIT)

Used by:
- objc 0.2.7 — http://github.com/SSheldon/rust-objc

## MIT License (SPDX: MIT)

Used by:
- tokio-util 0.7.18 — https://github.com/tokio-rs/tokio
- tokio 1.52.1 — https://github.com/tokio-rs/tokio

## MIT License (SPDX: MIT)

Used by:
- simd-adler32 0.3.9 — https://github.com/mcountryman/simd-adler32

## MIT License (SPDX: MIT)

Used by:
- endi 1.1.1 — https://github.com/zeenix/endi
- x11-dl 2.21.0 — https://github.com/AltF02/x11-rs.git
- x11 2.21.0 — https://github.com/AltF02/x11-rs.git
- zmij 1.0.21 — https://github.com/dtolnay/zmij
- zvariant_utils 3.3.1 — https://github.com/z-galaxy/zbus/

## MIT License (SPDX: MIT)

Used by:
- winnow 0.5.40 — https://github.com/winnow-rs/winnow
- winnow 0.7.15 — https://github.com/winnow-rs/winnow
- winnow 1.0.2 — https://github.com/winnow-rs/winnow

## MIT License (SPDX: MIT)

Used by:
- atk-sys 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- atk 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- cairo-rs 0.18.5 — https://github.com/gtk-rs/gtk-rs-core
- cairo-sys-rs 0.18.2 — https://github.com/gtk-rs/gtk-rs-core
- gdk-pixbuf-sys 0.18.0 — https://github.com/gtk-rs/gtk-rs-core
- gdk-pixbuf 0.18.5 — https://github.com/gtk-rs/gtk-rs-core
- gdk-sys 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gdk 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gdkwayland-sys 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gdkx11-sys 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gio-sys 0.18.1 — https://github.com/gtk-rs/gtk-rs-core
- gio 0.18.4 — https://github.com/gtk-rs/gtk-rs-core
- glib-macros 0.18.5 — https://github.com/gtk-rs/gtk-rs-core
- glib-sys 0.18.1 — https://github.com/gtk-rs/gtk-rs-core
- glib 0.18.5 — https://github.com/gtk-rs/gtk-rs-core
- gobject-sys 0.18.0 — https://github.com/gtk-rs/gtk-rs-core
- gtk-sys 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gtk3-macros 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- gtk 0.18.2 — https://github.com/gtk-rs/gtk3-rs
- pango-sys 0.18.0 — https://github.com/gtk-rs/gtk-rs-core
- pango 0.18.3 — https://github.com/gtk-rs/gtk-rs-core

## MIT License (SPDX: MIT)

Used by:
- javascriptcore-rs-sys 1.1.1 — https://github.com/tauri-apps/javascriptcore-rs
- soup3-sys 0.5.0 — https://gitlab.gnome.org/World/Rust/soup3-rs
- soup3 0.5.0 — https://gitlab.gnome.org/World/Rust/soup3-rs

## MIT License (SPDX: MIT)

Used by:
- javascriptcore-rs 1.1.2 — https://github.com/tauri-apps/javascriptcore-rs

## MIT License (SPDX: MIT)

Used by:
- zip 8.6.0 — https://github.com/zip-rs/zip2

## MIT License (SPDX: MIT)

Used by:
- freetype-sys 0.20.1 — https://github.com/PistonDevelopers/freetype-sys.git

## MIT License (SPDX: MIT)

Used by:
- aho-corasick 1.1.4 — https://github.com/BurntSushi/aho-corasick
- byteorder-lite 0.1.0 — https://github.com/image-rs/byteorder-lite
- byteorder 1.5.0 — https://github.com/BurntSushi/byteorder
- globset 0.4.18 — https://github.com/BurntSushi/ripgrep/tree/master/crates/globset
- memchr 2.8.0 — https://github.com/BurntSushi/memchr
- walkdir 2.5.0 — https://github.com/BurntSushi/walkdir

## MIT License (SPDX: MIT)

Used by:
- strsim 0.11.1 — https://github.com/rapidfuzz/strsim-rs

## MIT License (SPDX: MIT)

Used by:
- libxdo-sys 0.11.0 — https://github.com/crumblingstatue/rust-libxdo-sys
- libxdo 0.6.0 — https://github.com/crumblingstatue/rust-libxdo

## MIT License (SPDX: MIT)

Used by:
- fsevent-sys 4.1.0 — https://github.com/octplane/fsevent-rust/tree/master/fsevent-sys

## MIT License (SPDX: MIT)

Used by:
- y4m 0.8.0 — https://github.com/image-rs/y4m.git

## MIT License (SPDX: MIT)

Used by:
- data-encoding 2.11.1 — https://github.com/ia0/data-encoding

## MIT License (SPDX: MIT)

Used by:
- color_quant 1.1.0 — https://github.com/image-rs/color_quant.git

## MIT License (SPDX: MIT)

Used by:
- same-file 1.0.6 — https://github.com/BurntSushi/same-file

## MIT License (SPDX: MIT)

Used by:
- rust-embed-impl 8.11.0 — https://pyrossh.dev/repos/rust-embed
- rust-embed-utils 8.11.0 — https://pyrossh.dev/repos/rust-embed
- rust-embed 8.11.0 — https://pyrossh.dev/repos/rust-embed

## MIT License (SPDX: MIT)

Used by:
- yeslogic-fontconfig-sys 6.0.0 — https://github.com/yeslogic/fontconfig-rs

## MIT License (SPDX: MIT)

Used by:
- nu-ansi-term 0.50.3 — https://github.com/nushell/nu-ansi-term

## MIT License (SPDX: MIT)

Used by:
- generic-array 0.14.7 — https://github.com/fizyk20/generic-array.git

## MIT License (SPDX: MIT)

Used by:
- quick-xml 0.39.2 — https://github.com/tafia/quick-xml

## MIT License (SPDX: MIT)

Used by:
- urlencoding 2.1.3 — https://github.com/kornelski/rust_urlencoding

## MIT License (SPDX: MIT)

Used by:
- loop9 0.1.5 — https://gitlab.com/kornelski/loop9.git

## Mozilla Public License 2.0 (SPDX: MPL-2.0)

Used by:
- option-ext 0.2.0 — https://github.com/soc/option-ext.git

## Unicode License v3 (SPDX: Unicode-3.0)

Used by:
- unicode-ident 1.0.24 — https://github.com/dtolnay/unicode-ident

## Unicode License v3 (SPDX: Unicode-3.0)

Used by:
- icu_collections 2.2.0 — https://github.com/unicode-org/icu4x
- icu_locale_core 2.2.0 — https://github.com/unicode-org/icu4x
- icu_normalizer 2.2.0 — https://github.com/unicode-org/icu4x
- icu_normalizer_data 2.2.0 — https://github.com/unicode-org/icu4x
- icu_properties 2.2.0 — https://github.com/unicode-org/icu4x
- icu_properties_data 2.2.0 — https://github.com/unicode-org/icu4x
- icu_provider 2.2.0 — https://github.com/unicode-org/icu4x
- litemap 0.8.2 — https://github.com/unicode-org/icu4x
- potential_utf 0.1.5 — https://github.com/unicode-org/icu4x
- tinystr 0.8.3 — https://github.com/unicode-org/icu4x
- writeable 0.6.3 — https://github.com/unicode-org/icu4x
- yoke-derive 0.8.2 — https://github.com/unicode-org/icu4x
- yoke 0.8.2 — https://github.com/unicode-org/icu4x
- zerofrom-derive 0.1.7 — https://github.com/unicode-org/icu4x
- zerofrom 0.1.7 — https://github.com/unicode-org/icu4x
- zerotrie 0.2.4 — https://github.com/unicode-org/icu4x
- zerovec-derive 0.11.3 — https://github.com/unicode-org/icu4x
- zerovec 0.11.6 — https://github.com/unicode-org/icu4x

## zlib License (SPDX: Zlib)

Used by:
- zlib-rs 0.6.3 — https://github.com/trifectatechfoundation/zlib-rs

## zlib License (SPDX: Zlib)

Used by:
- const_format 0.2.36 — https://github.com/rodrimati1992/const_format_crates/
- const_format_proc_macros 0.2.34 — https://github.com/rodrimati1992/const_format_crates/

## zlib License (SPDX: Zlib)

Used by:
- konst 0.2.20 — https://github.com/rodrimati1992/konst/
- konst_macro_rules 0.2.19 — https://github.com/rodrimati1992/konst/

## zlib License (SPDX: Zlib)

Used by:
- slotmap 1.1.1 — https://github.com/orlp/slotmap

## zlib License (SPDX: Zlib)

Used by:
- foldhash 0.1.5 — https://github.com/orlp/foldhash
- foldhash 0.2.0 — https://github.com/orlp/foldhash


<!-- END cargo-about generated -->

## How this file is maintained

- The hand-curated NOTICE statement (Copyright, Apache 2.0 declaration) above is committed during P0 (project bootstrap).
- The third-party block between the `<!-- BEGIN cargo-about generated -->` and `<!-- END cargo-about generated -->` markers is regenerated mechanically by [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) against `Cargo.lock`. The CI gate at `.github/workflows/notice.yml` re-runs the generator on every PR that touches `Cargo.toml`, `Cargo.lock`, `about.toml`, the template, or this file, and fails the build on drift.
- To regenerate locally: `cargo install cargo-about --locked --features=cli && cargo about generate -c about.toml docs/about-template.hbs > /tmp/third-party.txt`, then replace the marked block with `/tmp/third-party.txt`.
- When upstream components are pinned to specific commits (e.g., `gpui-component`, pre-1.0), the pinned commit hashes are recorded in [`docs/upstream-watch.md`](docs/upstream-watch.md) for traceability.

## Trademarks

The names "dat0", "Accidentally Awesome Labs", and the `.dat0` file extension are not trademarks of any third party listed above. Use of those names by third parties is governed by Apache License 2.0 §6 (Trademarks).

External names referenced in this project (DuckDB, MotherDuck, GPUI, Apache Arrow, etc.) are the trademarks or registered trademarks of their respective owners and are used only descriptively.
