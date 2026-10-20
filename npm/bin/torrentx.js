#!/usr/bin/env node
// Runs torrentx from npm, npx, bunx, pnpm dlx or yarn dlx. On first use it
// downloads the release bundle named in release.json from GitHub, checks its
// SHA-256, unpacks it under %LOCALAPPDATA%\torrentx\versions\<version>, and
// then runs that torrentx.exe with the same arguments. Messages go to stderr
// only: stdout belongs to torrentx (the MCP server speaks JSON-RPC on it).
"use strict";

const { createHash } = require("node:crypto");
const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const release = require("../release.json");

function fail(message) {
  process.stderr.write(`torrentx: ${message}\n`);
  process.exit(1);
}

if (process.platform !== "win32" || process.arch !== "x64") {
  fail(`release bundles exist for Windows x64 only; this is ${process.platform} ${process.arch}.`);
}

const root = path.join(
  process.env.TORRENTX_HOME || path.join(process.env.LOCALAPPDATA || os.homedir(), "torrentx"),
  "versions",
);
const home = path.join(root, release.version);
const exe = path.join(home, "torrentx.exe");

async function download(target) {
  process.stderr.write(`torrentx: downloading ${release.version} (${Math.round(release.size / 1048576)} MB) once...\n`);
  const response = await fetch(release.url, { redirect: "follow" });
  if (!response.ok || !response.body) throw new Error(`download failed: HTTP ${response.status} from ${release.url}`);
  // Read it whole: pausing the body for backpressure trips an undici
  // assertion when the server closes the connection, and the bundle is small.
  const bytes = Buffer.from(await response.arrayBuffer());
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (digest !== release.sha256) {
    throw new Error(`the download's SHA-256 is ${digest}, expected ${release.sha256}; nothing was installed.`);
  }
  fs.writeFileSync(target, bytes);
}

async function install() {
  fs.mkdirSync(root, { recursive: true });
  // Leftovers of an install that was killed midway.
  for (const name of fs.readdirSync(root)) {
    if (name.startsWith(`.${release.version}-`)) {
      fs.rmSync(path.join(root, name), { recursive: true, force: true });
    }
  }
  const staging = fs.mkdtempSync(path.join(root, `.${release.version}-`));
  try {
    const zip = path.join(staging, "bundle.zip");
    await download(zip);
    // Windows 10 and later ship bsdtar, which reads zip files.
    const tar = path.join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe");
    const unpacked = spawnSync(tar, ["-xf", zip, "-C", staging], { stdio: ["ignore", "ignore", "inherit"] });
    if (unpacked.status !== 0) throw new Error("could not unpack the bundle with tar.exe.");
    const inner = path.join(staging, release.folder);
    if (!fs.existsSync(path.join(inner, "torrentx.exe"))) throw new Error("the bundle has no torrentx.exe.");
    try {
      fs.renameSync(inner, home);
    } catch (error) {
      // Another process installed the same version meanwhile.
      if (!fs.existsSync(exe)) throw error;
    }
    process.stderr.write(`torrentx: installed in ${home}\n`);
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
}

async function main() {
  if (!fs.existsSync(exe)) await install();
  const child = spawn(exe, process.argv.slice(2), { stdio: "inherit", windowsHide: false });
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => child.kill(signal));
  }
  child.on("error", (error) => fail(error.message));
  child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
}

main().catch((error) => fail(error.message));
