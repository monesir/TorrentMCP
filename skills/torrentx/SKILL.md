---
name: torrentx
description: Find torrents and verify what is really inside them with torrentx. Use when the user asks for a movie, series, anime or other release, asks which release is best, or wants to know a torrent file's real resolution, codecs, audio or subtitle tracks.
---

# torrentx

torrentx searches torrent sites, lists the files inside a torrent, and reads a file's real streams with ffprobe while downloading only its headers from peers. It never downloads whole files for the user.

## 1. Using torrentx

### Which interface

Use the MCP tools (`find`, `search`, `torrent_files`, `inspect_torrent`, `inspect_media`) when they are available. The server stays up as long as your session, keeps sources loaded and one torrent session warm, so a retry resumes a partial download instead of starting over.

Without MCP, use the command line and pipe the request on stdin. Do not write request files.

```powershell
'{"schema_version":1,"operation":"find","search":{"query":"The Bear S03"},"analysis":{"target":{"title":"The Bear","season":3}}}' | torrentx agent
```

On the command line every request carries `schema_version: 1` and `operation` (`find`, `search`, `torrent_files`, `inspect_torrent`, `inspect_media`); the MCP server adds both itself. Run `torrentx serve` in the background first if you will make more than one request: `agent` forwards to it and keeps the same warm session.

### Which tool

| The user wants | Call |
| --- | --- |
| A release, or the best release, of something | `find` |
| Only to see what exists, or names to choose from | `search` |
| What is inside a torrent they gave or you found | `torrent_files` |
| The real streams of specific files | `inspect_torrent` with `file_ids` |
| The real streams of a typical episode | `inspect_torrent` with `sample_files: 1` |
| The streams of a file already on this computer | `inspect_media` |

Start with `find` unless the user asked for something narrower. It searches, reads three files of each leading result (first, middle and last episode), compares what the files actually contain, and checks the file list for missing episodes, in one call.

### Building a request

Translate the user's words into fields; do not add wishes they did not express.

- `analysis.target`: the title, plus `year`, `season`, `part` or `episodes` when the user named them. When the user wants a whole season, put the season's full episode range from your research in `episodes` together with `season`: `find` then checks each pack's file list and reports the episodes it lacks in `coverage.missing`. Put other names the work is released under (romaji, English, original) in `aliases`.
- `analysis.constraints`: only what the user requires. A release whose name contradicts a constraint is excluded; one whose name says nothing is uncertain.
- `analysis.preferences`: ordered lists, best first. They rank and never exclude.
- `media.constraints` and `media.preferences`: the same idea checked on the inspected streams (`min_video_height`, `video_codecs`, `audio_languages`, `subtitle_languages`, bitrate and size bounds). `rank_by` sets which criterion decides first.
- Languages are ISO 639-1 codes: `en`, `ja`, `ar`.
- `search.category` narrows sources (`anime`, `tv`, `movies`, ...); the default is `all`. `search.sources` limits the search to source ids (`torrentx sources` lists them); omitted means every source.

Pass a search result's `reference` back exactly as it came. A magnet the user pasted needs only `{"kind": "magnet", "uri": "magnet:?..."}`.

### Time

- `search` takes seconds to about half a minute.
- `torrent_files` waits for the torrent's metadata from peers, up to `limits.timeout_seconds` (default 90).
- Each inspection then reads the file's headers, up to `inspection_limits.timeout_seconds` (default 60, at most 120).
- `find` therefore takes one to several minutes. Tell the user it is running; do not start a second identical call while the first is in progress.

### Reading the reply

Every reply has a `status`:

| `status` | Meaning |
| --- | --- |
| `completed` | Everything asked for was done. |
| `partial` | Real results, with some parts missing: a source failed, or some inspections failed. Use what is there and say what is missing. |
| `failed` | The work ran and nothing usable came of it; read `error`. |
| `error` | The request was rejected before any work, usually `invalid_request`; fix it from `error.message`. |
| `cancelled` | The call was stopped. |

The MCP server returns a compact view by default. Ask for `view: "full"` only when you need the per-token evidence behind a verdict; it is many times larger. `max_results` (default 20) controls how many search results the compact view shows.

In a compact `find` reply, each candidate either has `verified` (the container and streams ffprobe read from its file) and `unmet` (the conditions that file did not meet), or it has `inspection_error` and nothing else. Only candidates with `verified` have a `position`. A candidate may also carry `coverage`, the episodes found in its file names per season with `gaps` and `duplicates`, and `consistency`, how its other sampled files differ from the compared one; a difference such as an episode without the English subtitles is worth telling the user. `find` already retried a `probe_timeout` once by itself; a candidate with `attempts: 2` and still an `inspection_error` failed twice, so retry it with `inspect_torrent` only if it matters, with a larger timeout.

In `search.sources`, each source reports `completed` or `failed` with its error, and a failed one has a `failure_kind`. `search.source_failures` groups the failed sources by kind. Failed sources mean fewer results, not that the release does not exist.

### When something fails

| Code | What happened | What to do |
| --- | --- | --- |
| `probe_timeout` | Peers sent the file's header too slowly. | Retry once with `inspection_limits.timeout_seconds` raised (up to 120). The partial download is kept, so the retry continues it. |
| `torrent_service_timeout` | The torrent's metadata never arrived: no peer answered. | Retry once with a larger `limits.timeout_seconds`. If it fails again, the swarm is unreachable from here; the network may block BitTorrent. Say so; do not loop. |
| `read_budget_exceeded` | The headers were larger than `inspection_limits.max_read_bytes`. | Retry with a larger budget if the file matters. |
| `no_media_files` | The torrent has no video or audio files to inspect. | Report it. |
| `torrent_identity_mismatch`, `torrent_catalog_changed`, `torrent_metadata_invalid` | The torrent is not what the reference said, or is broken. | Do not retry; report it. |
| `torrent_service_unavailable`, `torrent_service_failed` | The local torrent engine failed. | Retry once; if it persists, report the message. |
| `invalid_request` | The request is malformed. | Fix the request from the message and call again. |

Source failures by `failure_kind`:

| Kind | Likely cause | What to say or do |
| --- | --- | --- |
| `dns_failed` | The name did not resolve through the system DNS nor through encrypted DNS: the domain is probably gone. | Say the site seems to be gone. |
| `connection_reset`, `network_unreachable` | The network blocks the site. | Say the site is blocked from this network. |
| `refused` | Bot protection (often Cloudflare). | Nothing to retry; the site refuses automated requests. |
| `rate_limited` | Too many requests. | Retry later, not now. |
| `not_found` | The site changed its search address. | Nothing to retry; torrentx needs an update for it. |
| `site_error` | The site or its proxy is down. | Retry later. |
| `timeout`, `tls_failed` | Slow, overloaded, or filtered. | One retry at most. |
| `not_configured` | Needs local setup (Jackett). | Ignore unless the user set it up. |

When an important source for the field fails (for anime, `nyaasi`), say so: the search is missing its main source, and the best release may not be in the results. Do not retry blocked sources in a loop.

A failed inspection means nothing about that file's streams is known. Say it was not inspected; do not fill the gap from the release name or from memory.

## 2. Know the field before searching

Release names, sources and quality differ by field. Before the first search, decide which field the request belongs to and read its file in this folder:

| The request is about | Read |
| --- | --- |
| Anime: TV series, OVA, ONA or anime films | [anime.md](anime.md) |
| Films (not anime) | [movies.md](movies.md) |
| TV series (not anime) | [series.md](series.md) |
| Music: albums, discographies, singles | [music.md](music.md) |

The field file tells you what to look up on the web about the work, which kinds of release exist, how they are named, and which sources carry them. Do that research first, then build the request from what you learned: the right titles and aliases, the right season and episode numbering, the right sources and constraints.

For a field with no file yet, still look the work up on the web first (official and alternative titles, year, seasons and episode counts) rather than searching on the user's wording alone.

In every field, a bigger file is not a better one. Codec efficiency, extra audio and subtitle tracks, disc structures and upscales all change size without changing quality the same way. Compare verified streams against what the user asked for.

Keep what the web told you apart from what torrentx verified, and say which is which when you report.
