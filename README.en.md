# torrentx

[العربية](README.md)

torrentx is a local torrent search and inspection tool built for AI agents. It searches torrent sites, lists the files inside a torrent, and reads a file's real streams with ffprobe while downloading only the file's headers from peers. An agent can then answer "which release of X is best at 1080p with English subtitles" from what the files actually contain, not from what their names say.

It runs on the user's machine. Nothing is uploaded, and no whole file is ever downloaded for the user.

## Claimed and verified

torrentx keeps two kinds of facts apart:

- **Claimed**: resolution, codec, source and audio read from a release name. A name can be wrong or incomplete.
- **Verified**: containers, codecs, resolution, bit depth, HDR, audio and subtitle tracks read by ffprobe from the file itself.

A failed inspection means nothing about that file is known. torrentx never fills the gap from the name.

## Install (Windows)

Download the zip from the latest [release](https://github.com/monesir/TorrentMCP/releases/latest) (about 40 MB; its SHA-256 is published beside it):

1. Unzip `torrentx-<version>-windows-x64.zip`, for example into `%LOCALAPPDATA%\torrentx`. The bundle carries its own Python and ffprobe; nothing else needs installing. Keep `torrentx.exe`, `resources`, `runtime` and `skills` together.
2. Register torrentx with your agents:

   ```powershell
   .\torrentx.exe setup
   ```

   setup finds the installed agents (Claude Code, Codex, Antigravity, Claude Desktop, Gemini CLI, Cursor), asks for each one whether to add torrentx, and whether to install the torrentx skill. It shows every change before applying it and backs up each file it edits. Restart the agent afterwards.

Details, flags and manual registration: [MCP server](docs/mcp.en.md).

## Using it from an agent

Once registered, the agent has five MCP tools:

| Tool | Does |
| --- | --- |
| `find` | Searches, reads three episodes of each leading release, compares their real streams, and checks the file list for missing episodes. Start here. |
| `search` | Searches and judges release names only. |
| `torrent_files` | Lists a torrent's files. |
| `inspect_torrent` | Inspects chosen files of a torrent, or a sample. |
| `inspect_media` | Inspects a media file already on disk. |

The agent states what "best" means through constraints (must have) and preferences (ordered, best first); torrentx assumes nothing else. `find` takes one to several minutes because it waits for peers. An inspection that times out is retried once automatically, resuming the same partial download.

The skill installed by setup teaches the agent how to call these tools, what each failure means, and what to research on the web first for anime, films, series and music.

## Command line

Every operation is also a JSON request on stdin:

```powershell
'{"schema_version":1,"operation":"find","search":{"query":"The Bear S03"},"analysis":{"target":{"title":"The Bear","season":3}}}' | .\torrentx.exe agent
```

```powershell
.\torrentx.exe sources                         # source ids, scopes, and retired sites
.\torrentx.exe search "ubuntu" --limit 20      # quick human-readable search
.\torrentx.exe agent --request .\examples\find.json --format markdown --output report.md
.\torrentx.exe serve                           # keep sources and the torrent session warm
```

`agent` forwards to a running `serve` automatically, so repeated requests reuse one warm session. The full request and reply format is in the [agent contract](docs/agent-contract.md) (Arabic).

## Sources

torrentx ships Rust ports of 86 qBittorrent search plugins (8 official, 78 community), with the original Python plugins kept as a selectable fallback. Each plugin file is pinned by its SHA-256. By default a search uses the sources suited to its category; adult, single-language, duplicate and retired sources run only when named.

When the system DNS cannot resolve a site, torrentx asks Cloudflare and Google over encrypted DNS (DoH). Set `TORRENTX_ENCRYPTED_DNS=off` to disable this. Each failed source reports a `failure_kind` (`dns_failed`, `connection_reset`, `refused`, ...) so the agent can tell a blocked network from a broken site.

More: [native sources](docs/native-sources.md).

## Documentation

| Topic | English | Arabic |
| --- | --- | --- |
| MCP server and setup | [mcp.en.md](docs/mcp.en.md) | [mcp.md](docs/mcp.md) |
| Agent contract | | [agent-contract.md](docs/agent-contract.md) |
| Media inspection | | [media-inspection.md](docs/media-inspection.md) |
| Media comparison | | [media-comparison.md](docs/media-comparison.md) |
| Torrent workflow | | [torrent-workflow.md](docs/torrent-workflow.md) |
| Background service | | [daemon.md](docs/daemon.md) |
| Markdown reports | | [markdown-reports.md](docs/markdown-reports.md) |

## Development

Rust, Python and ffprobe are needed to run the whole test suite; qBittorrent is not.

```powershell
cargo test --locked
cargo run -- --assets resources --python python sources
```

`TORRENT_SEARCH_TEST_PYTHON` and `TORRENT_SEARCH_TEST_FFPROBE` point the tests at explicit executables.

Build the portable bundle (runs the tests, builds release, gathers Python, ffprobe, resources, the skill and licences, then writes a ZIP with its checksum under `dist`):

```powershell
.\scripts\package-windows.ps1 -FFprobePath <ffprobe-dir>\bin\ffprobe.exe
```

Release bundles use a minimal static ffprobe (17 MB, LGPL) built by `scripts/build-ffprobe.sh` from signed FFmpeg source in an MSYS2 UCRT64 shell; any ffprobe works for development.

Component licences: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## License

MIT: see [LICENSE](LICENSE). Bundled components keep their own licences.
