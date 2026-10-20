// Write release.json for a packaged bundle and set package.json's version.
// Usage: node npm/stamp-release.js dist/torrentx-<version>-windows-x64.zip
"use strict";

const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const zip = process.argv[2];
const match = /^torrentx-(\d+\.\d+\.\d+)-windows-x64\.zip$/.exec(path.basename(zip || ""));
if (!match) throw new Error("expected dist/torrentx-<version>-windows-x64.zip");
const version = match[1];
const bytes = fs.readFileSync(zip);
const release = {
  version,
  folder: `torrentx-${version}-windows-x64`,
  url: `https://github.com/monesir/TorrentMCP/releases/download/v${version}/torrentx-${version}-windows-x64.zip`,
  sha256: createHash("sha256").update(bytes).digest("hex"),
  size: bytes.length,
};
fs.writeFileSync(path.join(__dirname, "release.json"), JSON.stringify(release, null, 2) + "\n");
const manifest = path.join(__dirname, "package.json");
const pkg = JSON.parse(fs.readFileSync(manifest, "utf8"));
pkg.version = version;
fs.writeFileSync(manifest, JSON.stringify(pkg, null, 2) + "\n");
console.log(JSON.stringify(release, null, 2));
