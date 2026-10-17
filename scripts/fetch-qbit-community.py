"""Snapshot the official community wiki and its linked Python sources.

Downloads are research material only: never import, run, or enable them here.
Every row is retained, including duplicates and unavailable/non-file links.
Clone the wiki into .cache/qbit-plugin-wiki before running this command.
"""
import ast
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import subprocess
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
WIKI = ROOT / ".cache/qbit-plugin-wiki"
OUTPUT = ROOT / "research/qbittorrent/community-20261017"


def links(cell):
    return re.findall(r"(?<!\[)\[(https?://[^\s\]]+)\s*([^\[\]]*)", cell)


def fetch(entry):
    if entry["visibility"] == "private":
        return dict(entry, in_scope=False, fetch_status="excluded_by_user", native_status="out_of_scope")
    url = entry["download_url"]
    if not url or not re.search(r"\.py(?:[?#]|$)", url):
        return dict(entry, fetch_status="manual_source_selection_required")
    # GitHub blob links refer to an HTML page, not the Python file.
    if url.startswith("https://github.com/") and "/blob/" in url:
        url = url.replace("https://github.com/", "https://raw.githubusercontent.com/", 1).replace("/blob/", "/", 1)
    try:
        with urlopen(Request(url, headers={"User-Agent": "torrent-search-source-audit/1"}), timeout=12) as response:
            payload = response.read(1024 * 1024 + 1)
            final_url = response.url
        if len(payload) > 1024 * 1024:
            raise ValueError("Source exceeds 1 MiB")
        ast.parse(payload)  # Validate as data; do not import third-party code.
        digest = hashlib.sha256(payload).hexdigest()
        path = OUTPUT / "sources" / f'{entry["entry_id"]}.py'
        path.write_bytes(payload)
        return dict(entry, fetch_status="downloaded_not_reviewed", sha256=digest,
                    fetched_url=final_url, local_path=str(path.relative_to(ROOT)).replace("\\", "/"),
                    bytes=len(payload), license_status="pending_upstream_review", rust_status="pending")
    except (HTTPError, URLError, OSError, SyntaxError, ValueError) as error:
        return dict(entry, fetch_status="unavailable", error=f"{type(error).__name__}: {error}")


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "sources").mkdir(exist_ok=True)
    text = (WIKI / "Unofficial-search-plugins.mediawiki").read_text(encoding="utf-8")
    commit = subprocess.check_output(["git", "-C", str(WIKI), "rev-parse", "HEAD"], text=True).strip()
    (OUTPUT / "Unofficial-search-plugins.mediawiki").write_text(text, encoding="utf-8")
    entries = []
    for section, next_section, visibility in [("== Plugins for Public Sites ==", "== Plugins for Private Sites ==", "public"),
                                             ("== Plugins for Private Sites ==", None, "private")]:
        table = text.split(section, 1)[1]
        if next_section:
            table = table.split(next_section, 1)[0]
        for row in table.split("|-\n"):
            cells = [line[2:] for line in row.splitlines() if line.startswith("| ")]
            if len(cells) < 6:
                continue
            site = links(cells[0])
            author = links(cells[1])
            downloads = links(cells[4])
            download = next((url for url, _ in downloads if re.search(r"\.py(?:[?#]|$)", url)), downloads[0][0] if downloads else None)
            entries.append(dict(entry_id=f"{len(entries)+1:03d}", visibility=visibility, in_scope=visibility == "public",
                                name=site[-1][1].strip() if site else cells[0], site_url=site[-1][0] if site else None,
                                upstream_repository=author[0][0] if author else None,
                                listed_version=cells[2], listed_update=cells[3], download_url=download,
                                notes=cells[5], native_status="pending", enabled=False))
    if not entries or not any(row["visibility"] == "private" for row in entries):
        raise ValueError("Wiki structure changed; inspect before importing")
    # Save the inventory before network requests, so an interrupted run is resumable.
    manifest = dict(wiki_commit=commit, wiki_url="https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins",
                    entries=entries)
    manifest_path = OUTPUT / "catalog.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    with ThreadPoolExecutor(max_workers=6) as executor:
        records = list(executor.map(fetch, entries))
    manifest["entries"] = records
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    statuses = {status: sum(row["fetch_status"] == status for row in records)
                for status in sorted({row["fetch_status"] for row in records})}
    print(json.dumps(dict(entries=len(records), public=sum(row["visibility"] == "public" for row in records),
                          private=sum(row["visibility"] == "private" for row in records), statuses=statuses), ensure_ascii=False))


if __name__ == "__main__":
    main()
