# Movies

Read this before searching for a film (not anime; see [anime.md](anime.md)).

## 1. Research the film on the web

TMDB and IMDb cover almost every film. Note your sources: these are web facts, not torrentx verification.

- **Title and year:** the original title and the English one, and the release year. The year separates remakes and same-named films; put it in `analysis.target.year`.
- **Release status:** still only in cinemas, out on digital platforms, or out on disc. A film only in cinemas exists only as camera recordings (CAM, TS, TC), which the default constraints reject; tell the user instead of offering them.
- **Editions:** theatrical, director's cut, extended, unrated, IMAX. Releases name the edition; make sure the one you offer is the one the user wants.
- **Original language and dubs:** which language the film was made in, and whether the user wants it or a dub.
- **Disc formats:** whether a 4K UHD Blu-ray exists, and whether the film is HDR or Dolby Vision there. A 2160p release of a film without a 4K master is an upscale.

## 2. Kinds of release

From worst to best picture, roughly:

| Kind | What it is | Name signs |
| --- | --- | --- |
| Camera | Filmed in a cinema. | `CAM`, `TS`, `HDTS`, `TC`, `TELESYNC` |
| Screener | A promotional copy, often watermarked. | `SCR`, `DVDSCR` |
| TV | Recorded from broadcast. | `HDTV` |
| WEBRip | Captured or re-encoded from a stream. | `WEBRip` |
| WEB-DL | The stream's own file, not re-encoded. | `WEB-DL`, `WEB`, a service tag (`AMZN`, `NF`, `DSNP`, `ATVP`, `MAX`) |
| Blu-ray encode | Re-encoded from the disc. | `BluRay`, `BDRip`, `x264`, `x265` |
| Remux | The disc's video and audio untouched, in an MKV. | `Remux` |
| Full disc | The disc structure itself; not a single playable file. | `BDMV`, `ISO`, `COMPLETE.BLURAY` |

Encode quality depends on the group, the codec and the bitrate, not on the kind alone. A good WEB-DL can beat a poor Blu-ray encode.

The largest is not the best. A remux is the disc's quality at the disc's size; a careful encode at a third of the size can look nearly the same. A full disc is the biggest and the least convenient. Several dub tracks inflate size without improving the picture. Some groups are known for very small files with low bitrates: they are small because quality was given up.

## 3. Picture and sound details

- **HDR:** `HDR`, `HDR10`, `HDR10+`, `DV` (Dolby Vision) in the name. Dolby Vision without an HDR10 base layer shows wrong colours on players that do not support it. Inspection reports `hdr_signals` (`pq_transfer`, `hlg_transfer`, `dolby_vision_metadata`, `mastering_display_metadata`); they are verified, the name's claims are not.
- **Audio:** `TrueHD`/`Atmos` and `DTS-HD MA` are lossless disc tracks; `DDP`/`EAC3` and `AAC` are streaming tracks; `DTS` and `AC3` are older lossy tracks. Inspection shows the codec, channels and language of every track.
- **Languages:** `MULTI`, `DUAL`, or a language name means more than one audio track. Check the verified languages, not the tag.

## 4. Sources

General sites carry most films. `yts` has only small encodes. Specialised sites by language (Spanish, French, Russian, Brazilian) carry dubbed releases in that language: include them only when the user wants that language.

## 5. From the research to the request

- `search.query`: the title and year, for example `Dune Part Two 2024`. Use `category: "movies"`.
- `analysis.target`: `title`, `year`, and the other title in `aliases`.
- Resolution and source format the user requires go in `analysis.constraints`; what they only prefer goes in `analysis.preferences`.
- A required audio language or subtitle language goes in `media.constraints`, so it is checked on the real tracks.
- A film is usually one file, so `find` inspects that file. Check the verified height, HDR signals and audio tracks against what the user asked for.
