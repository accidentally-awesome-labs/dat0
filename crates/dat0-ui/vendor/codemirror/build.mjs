// Builds `src/index.js` into `../../assets/codemirror.js`.
//
// Run by hand, never by cargo — CI has no Node and `xtask` bundling must stay a
// pure `cargo build`. The produced bundle is committed. See README.md.

import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(here, "../../assets/codemirror.js");

const result = await build({
  entryPoints: [resolve(here, "src/index.js")],
  outfile: out,
  bundle: true,
  minify: true,
  format: "iife",
  platform: "browser",
  // WKWebView on the oldest macOS dat0 supports, and WebKitGTK on Linux.
  target: ["safari15", "chrome100"],
  legalComments: "none",
  metafile: true,
  logLevel: "info",
});

const bytes = Object.values(result.metafile.outputs)[0].bytes;
console.log(`wrote ${out} (${(bytes / 1024).toFixed(1)} KiB)`);
