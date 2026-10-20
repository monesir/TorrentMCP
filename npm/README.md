# torrentx-mcp

torrentx searches torrent sites, lists the files inside a torrent, and reads a file's real streams (resolution, codecs, HDR, audio and subtitle tracks) with ffprobe while downloading only the file's headers. It is built for AI agents, as an MCP server.

Windows x64 only for now. The first run downloads the release bundle (about 40 MB) from [GitHub Releases](https://github.com/monesir/TorrentMCP/releases), checks its SHA-256 and unpacks it under `%LOCALAPPDATA%\torrentx\versions`.

## Set it up for your agents

```powershell
npx torrentx-mcp setup
```

or `bunx torrentx-mcp setup`. It finds Claude Code, Codex, Antigravity, Claude Desktop, Gemini CLI and Cursor, asks which ones to register torrentx with, and whether to install the torrentx skill. Restart the agent afterwards.

To register the server by hand in any MCP client:

```json
{ "mcpServers": { "torrentx": { "command": "npx", "args": ["-y", "torrentx-mcp", "mcp"] } } }
```

## Other commands

```powershell
npx torrentx-mcp sources            # the search sources
npx torrentx-mcp search "ubuntu"    # a quick search
```

Documentation: [github.com/monesir/TorrentMCP](https://github.com/monesir/TorrentMCP).
