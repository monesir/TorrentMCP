# MCP server

[العربية](mcp.md)

`torrentx mcp` serves torrentx's operations as MCP tools over stdin/stdout. An agent calls them directly: no request files, no terminal commands. The server lives as long as its client, keeps sources loaded and one rqbit session warm, so re-inspecting a file resumes the partial download instead of starting over.

## Install

```powershell
torrentx setup
```

It detects the installed agents, shows every change, and applies nothing until you approve. Each file is backed up before it is edited (`<file>.torrentx-backup-<time>`). Restart the agent afterwards.

| Agent | Id | What setup does |
| --- | --- | --- |
| Claude Code | `claude_code` | `claude mcp add --scope user torrentx -- <torrentx.exe> mcp` |
| Codex | `codex` | `codex mcp add torrentx -- <torrentx.exe> mcp` |
| Antigravity | `antigravity` | adds `torrentx` to `mcpServers` in `~/.gemini/config/mcp_config.json` |
| Claude Desktop | `claude_desktop` | `%APPDATA%\Claude\claude_desktop_config.json` |
| Gemini CLI | `gemini_cli` | `~/.gemini/settings.json` |
| Cursor | `cursor` | `~/.cursor/mcp.json` |

Claude Code and Codex are configured through their own CLIs, because each writes its configuration file while it runs. Without the `claude` CLI on PATH, setup edits `~/.claude.json` directly.

```powershell
torrentx setup --client antigravity,codex   # only these agents
torrentx setup --yes                        # do not ask
torrentx setup --remove                     # remove the registration
```

The registered path is the `torrentx.exe` setup ran from. Run setup again after moving the bundle.

### Manual registration

For any other MCP client, the command is the full path of `torrentx.exe` and the argument is `mcp`.

```json
{
  "mcpServers": {
    "torrentx": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\torrentx\\torrentx.exe",
      "args": ["mcp"]
    }
  }
}
```

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.torrentx]
command = 'C:\Users\<you>\AppData\Local\torrentx\torrentx.exe'
args = ["mcp"]
```

## Tools

| Tool | Does |
| --- | --- |
| `find` | Search, inspect one typical file of each leading candidate, compare their actual streams. Start here. |
| `search` | Search and judge release names only; everything in it is claimed by names. |
| `torrent_files` | A torrent's files and their ids. |
| `inspect_torrent` | Inspect chosen files (`file_ids`) or a sample (`sample_files`). |
| `inspect_media` | Inspect a local media file. |

Arguments are the fields of the [agent contract](agent-contract.md) without `operation` and `schema_version`, which the server adds. `search.category` defaults to `all` and `search.sources` to every source, here and on the command line.

### Replies

By default the server returns a compact view: the first 20 search results (`max_results`), each with title, reference, size, seeders, eligibility, reasons for exclusion, and what the name claims under `claimed`. File lists show video and audio files only (`max_files`, default 200). Empty values and container tags are dropped. A 20-result search is about 20 KB instead of 270 KB in full. `view: "full"` returns the complete agent response.

A compact `find` lists each candidate with its own evidence: `verified` holds the container and streams ffprobe read from its file, `unmet` the conditions that file did not meet. A candidate whose file was not read has no `verified` and no `position`, only `inspection_error`.

`isError` is true for `error`, `failed` and `cancelled`. `partial` is not an error: the results are real and some are missing.

### What the agent reads on connecting

The server sends instructions that keep two kinds of facts apart:
- **Claimed:** everything in search results comes from release names and can be wrong.
- **Verified:** streams in a completed inspection come from the file itself.

A failed inspection (`probe_timeout`, for example) means nothing about that file's streams is known. The agent retries with a larger timeout (up to 120 s), and the server keeps what was downloaded. A `find` candidate that was not inspected gets no ranking position.
