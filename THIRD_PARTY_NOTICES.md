# Third-party source and runtime notices

The qBittorrent application is not included or required. Selected search-engine
support files and plugins are included under their original notices.

- `resources/qbit/helpers.py` and `novaprinter.py`: copied unchanged from the
  inspected qBittorrent 5.2.3 search runtime. Each retains its BSD-style notice.
  Matching upstream commit: `0b63c3d17373f6132ea211c9dcd4241284ccdfaf`.
- `resources/qbit/socks.py`: bundled PySocks 1.7.1; the original notice is retained.
- `resources/qbit/engines/torrentscsv.py` 1.8: original BSD-style notice retained.
- `resources/qbit/engines/limetorrents.py` 4.15: from the qBittorrent search-plugin
  ecosystem; its repository license is provided as `licenses/search-plugins-COPYING`.
  The original source and author attribution remain in the resource directory.
- Provenance and SHA-256 for the inspected originals:
  `research/qbittorrent/snapshot.manifest.json`. Runtime file pins are in
  `resources/sources.json`.
- The current main-repository plugin snapshot is pinned to commit
  `240b57a90ca4f9e3869151b73c76c46f1b7bb59b` under
  `research/qbittorrent/upstream-20261017`. Retained plugin files preserve their
  original author and license notices, including file-specific BSD/MIT notices.
- Reviewed public community variants are mapped to original download URLs,
  SHA-256 digests and catalog entry IDs in
  `research/qbittorrent/community-20261017/catalog.json`. The full wiki snapshot
  also lists private entries, which are excluded from runtime registration.
  Only explicitly registered public files are copied into `resources/qbit/engines`.
  Native ports preserve the original attribution and protocol provenance;
  the compatibility worker, rather than the original files, supplies legacy
  helper aliases and reference normalization.
- BurningMop public plugin source files retain their MIT permission notice.
  LightDestory community sources are covered by their upstream GPL-3.0 notice.
  Nyaa/Sukebei, Anime Tosho, acgrip and mikanani retain their file-level GPLv3-or-later
  notices. Recovered Pantsu variants have no declared upstream license in the
  inspected archived repository; their license status is recorded separately
  rather than inferred from qBittorrent's license.
- Windows release bundles include the official CPython 3.14.8 embeddable runtime.
  Its license and bundled notices remain in `runtime`.
- Python compatibility wheels (Beautiful Soup, lxml, requests and their dependencies) are pinned with PyPI-verified SHA-256 digests and original download URLs in `resources/python-dependencies.lock.json`. Their dist-info metadata and license files remain in `runtime/Lib/site-packages`.
- Native-source bundles include the corresponding local Rust source, Cargo manifests, tests and build scripts in `source/`; original Python plugins and their notices remain in `resources/`.
- Rust dependency versions are recorded in `Cargo.lock`. The packaging workflow
  copies their supplied top-level license/notice files into
  `licenses/rust-dependencies`.
- Windows 0.3 release bundles also include an unmodified ffprobe executable and
  its companion DLLs from the locally selected FFmpeg Windows distribution.
  Upstream `LICENSE` and `README.txt` (build configuration, attribution and
  corresponding-source link) are retained in `runtime/media`. The build used
  here is Gyan FFmpeg 9.0.1 shared, GPL v3. Exact version, provenance and file
  SHA-256 values are recorded in `package-info.json`. FFmpeg remains a separate
  subprocess. No ffmpeg transcoding executable is included.
- From 0.8.6 the bundled ffprobe is a minimal static build of unmodified FFmpeg
  9.0.2 source made by `scripts/build-ffprobe.sh`: no external libraries, no GPL
  or nonfree components, so it is under LGPL v2.1 or later. Its `LICENSE`
  (COPYING.LGPLv2.1) and `README.txt` (source URL and SHA-256, signing key,
  compiler and configure line) are in `runtime/media`. Source:
  https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz .

- Windows 0.5 bundles include the unmodified rqbit v9.0.1 executable as a separate
  local torrent-service process, under Apache-2.0. Its upstream LICENSE remains in
  `runtime/torrent/LICENSE`. Origin: https://github.com/ikatson/rqbit/releases/tag/v9.0.1 .
  The pinned executable digest is recorded in `package-info.json`.

The project's own implementation and third-party files have separate provenance;
this notice does not replace the original license terms.

