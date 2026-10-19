# Anime

Read this before searching for anime. Anime releases are named, numbered and distributed differently from Western films and series, and a search built from the user's wording alone often misses the best releases or matches the wrong season.

## 1. Research the work on the web

Look the work up before the first search. AniList, MyAnimeList and AniDB cover most titles; AniDB is the most precise for episode lists and for which groups released what. Note your sources: these are web facts, not torrentx verification.

Find out:

- **Every title it is released under:** Japanese, romaji and English. Release groups mostly use romaji (`Shingeki no Kyojin`), Western sites the English title (`Attack on Titan`). Put the ones you do not use as `query` in `analysis.target.aliases`.
- **Type and structure:** TV, OVA, ONA or film; how many seasons and cours; episodes per season. A "season 2" may be released under a separate title (`... 2nd Season`, `... Final Season`) or numbered on from season 1 (episode 13 instead of S02E01).
- **Status:** still airing, finished, or announced. For an airing show, which episodes are out and when the next one airs.
- **Home video:** whether a Blu-ray exists or is announced. Releases made before the Blu-ray are from broadcast or streaming.
- **Production resolution, when the user cares about quality:** many TV anime are produced below 1080p and scaled up, and older works are SD. A 1080p file of such a work is an upscale; ffprobe reads the file's size, not the resolution it was drawn at.

## 2. Kinds of release

| Kind | What it is | Typical name signs |
| --- | --- | --- |
| TV raw | Japanese broadcast, no subtitles; may carry channel logos or a cropped picture. | `RAW`, a broadcaster's name, `TS`, `-Raws` group |
| Web raw | A Japanese streaming service, no subtitles. | `RAW`, `WEB`, the service name |
| BD raw | From the Blu-ray, no subtitles; usually the best picture. | `BDRip`, `BD`, `-Raws` group |
| BDMV / disc | The whole Blu-ray disc structure, very large, not a playable file per episode. | `BDMV`, `ISO`, `DISC` |
| Simulcast rip | An official stream with its subtitles (often English), soon after airing. | `WEB-DL`, `WEBRip`, the service name |
| Fansub | Subtitles made by a group, usually in its own language. | `[Group]` in front, language tags |
| BD encode / remux | A Blu-ray re-encoded or remuxed, often with several subtitle tracks and sometimes dubs. | `BD`, `Remux`, `Dual Audio`, `Multi-Subs` |
| Batch | A whole season or series in one torrent. | `Batch`, `01-12`, `Complete` |

Which groups are active changes over the years: some stop, others start. Do not rely on memory; check on the web or on nyaa which groups released this work, and in which kind.

The largest release is not the best one. A BDMV is huge but is a disc, not episodes to play. An HEVC 10-bit encode can match a much larger H.264 one. Extra dubs and subtitle tracks add size without adding picture. And an upscale from a low production resolution is large but no sharper. Judge by the verified streams and by what the user asked for, never by size alone.

Ask yourself what the user wants: raw (no subtitles), subtitled in a language, dubbed, or the best picture. If it is not clear and it changes the search, ask once.

## 3. How names look

Typical: `[Group] Title - 05 (1080p) [ABCD1234].mkv`, a batch `[Group] Title (01-12) [BD 1080p]`.

- The season often does not appear as `S02`; it is in the title or in the episode number. If `season` would exclude good results, search by title and use `episodes` instead.
- The bracketed hex code at the end is a checksum, not a version.
- `v2` after an episode number is a corrected re-release; prefer it over the original.

## 4. Sources

Search with `category: "anime"`. For a focused search, name anime sources in `search.sources`:

| Source id | Notes |
| --- | --- |
| `nyaasi` | The main anime tracker; most groups publish here first. |
| `animetosho` | Mirrors nyaa and other trackers with added file details. |
| `tokyotoshokan`, `anidex`, `nekobt` | Other anime trackers. |
| `subsplease` | One group's simulcast rips only. |
| `dmhy`, `mikan`, `acgrip` | Chinese sites; releases there mostly carry Chinese subtitles. |

Leave general sites in only when the anime sources return nothing useful.

## 5. From the research to the request

- `search.query`: the romaji or English title, whichever groups use for this work; the other goes in `aliases`.
- `analysis.target`: `title`, `aliases`, and `season` or `episodes` as the research showed they are numbered.
- Subtitles in a language: `media.constraints.subtitle_languages`, for example `["en"]`.
- Japanese audio: `media.preferences.audio_languages: ["ja"]`. Raw tracks often carry no language tag; that reads as unknown, not as a mismatch.
- Raw: there is no field for "no subtitles". Inspect, then check that the verified streams contain no subtitle track.

## 6. What inspection can and cannot show

- It shows the file's real resolution, codec, bit depth, audio tracks and subtitle tracks.
- It cannot see subtitles burned into the picture (hardsubs). A file with no subtitle stream may still have hardsubs; say so when it matters, especially for Chinese-site releases and some simulcast rips.
- It cannot tell native resolution from an upscale. Compare the verified height with the production resolution you found on the web, and present that comparison as your reasoning, not as a measurement.
