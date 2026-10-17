"""Reproduce the bounded qBittorrent search-plugin compatibility investigation.

This is a research probe, not the application's search service. It copies only
the explicitly selected Python files; it never loads the user's qBittorrent
configuration or starts a BitTorrent download. License headers remain intact.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import urllib.parse
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parent
SNAPSHOT = ROOT / "snapshot"
PLUGINS = ("torrentscsv", "limetorrents")
CORE_FILES = ("nova2.py", "nova2dl.py", "helpers.py", "novaprinter.py", "socks.py")


def run_plugin(plugin: str, query: str, resolve: str | None = None) -> None:
    """Execute one reviewed plugin in a separate interpreter, with bounded I/O."""
    sys.path.insert(0, str(SNAPSHOT))
    socket.setdefaulttimeout(12)
    module = importlib.import_module(f"engines.{plugin}")
    engine = getattr(module, plugin)()
    if resolve is not None:
        engine.download_torrent(resolve)
    else:
        engine.search(urllib.parse.quote(query), "all")


def capture_process(arguments: list[str], timeout: int) -> dict:
    environment = dict(os.environ)
    for key in tuple(environment):
        if key.upper().startswith("PYTHON") or key.lower() in {"qbt_socks_proxy", "sock_proxy"}:
            environment.pop(key)
    started = time.monotonic()
    process = subprocess.Popen(
        [sys.executable, "-I", "-B", "-X", "utf8", *arguments],
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        process.kill()
        stdout, stderr = process.communicate()
    return {
        "exit_code": process.returncode,
        "timed_out": timed_out,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "stdout": stdout.decode("utf-8", errors="replace"),
        "stderr": stderr.decode("utf-8", errors="replace"),
    }


def inspect_rows(output: str) -> dict:
    rows = []
    malformed_lines = []
    for line in output.splitlines():
        fields = line.split("|")
        if len(fields) != 8:
            malformed_lines.append(line)
            continue
        link, name, size, seeds, leech, site, description, published = fields
        try:
            rows.append({
                "link": link,
                "name": name,
                "size_bytes": int(size),
                "seeders": int(seeds),
                "leechers": int(leech),
                "site": site,
                "description": description,
                "published_unix": int(published),
                "reference_kind": "magnet" if link.startswith("magnet:") else "http",
            })
        except ValueError:
            malformed_lines.append(line)
    return {"count": len(rows), "sample": rows[:3], "malformed_lines": malformed_lines}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", choices=PLUGINS, help=argparse.SUPPRESS)
    parser.add_argument("--query", default="ubuntu")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--resolve", help=argparse.SUPPRESS)
    parser.add_argument("--show-results", action="store_true")
    parser.add_argument("--resolve-sample", action="store_true")
    arguments = parser.parse_args()
    if arguments.worker:
        run_plugin(arguments.worker, arguments.query, arguments.resolve)
        return 0
    if arguments.resolve_sample:
        search_report = json.loads((ROOT / "probe.results.json").read_text(encoding="utf-8"))
        source_link = search_report["searches"]["limetorrents"]["parsed"]["sample"][0]["link"]
        result = capture_process([str(Path(__file__).resolve()), "--worker", "limetorrents", "--resolve", source_link], 20)
        result["source_link"] = source_link
        result["returned_magnet"] = result["stdout"].strip().startswith("magnet:")
        result["observed_at_utc"] = datetime.now(timezone.utc).isoformat()
        (ROOT / "resolution.results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps({key: result[key] for key in ("exit_code", "timed_out", "elapsed_seconds", "returned_magnet")}, indent=2))
        return 0 if result["exit_code"] == 0 and result["returned_magnet"] else 1
    if arguments.show_results:
        report = json.loads((ROOT / "probe.results.json").read_text(encoding="utf-8"))
        print(json.dumps({
            "discovered_plugins": report["discovered_plugins"],
            "searches": {name: {"status": result["status"], "count": result["parsed"]["count"], "exit_code": result["exit_code"], "seconds": result["elapsed_seconds"]} for name, result in report["searches"].items()},
        }, ensure_ascii=True, indent=2))
        return 0
    source = arguments.source or Path(os.environ["LOCALAPPDATA"]) / "qBittorrent" / "nova3"
    (SNAPSHOT / "engines").mkdir(parents=True, exist_ok=True)
    manifest = []
    for relative in [*CORE_FILES, "__init__.py", "engines/__init__.py", *[f"engines/{name}.py" for name in PLUGINS]]:
        original = source / relative
        destination = SNAPSHOT / relative
        shutil.copyfile(original, destination)
        content = destination.read_bytes()
        manifest.append({
            "relative_path": relative,
            "source": str(original),
            "bytes": len(content),
            "sha256": hashlib.sha256(content).hexdigest(),
            "header": content.decode("utf-8-sig").splitlines()[:4],
        })
    (ROOT / "snapshot.manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    capabilities = capture_process([str(SNAPSHOT / "nova2.py"), "--capabilities"], 10)
    discovered = []
    if capabilities["exit_code"] == 0:
        discovered = [node.tag for node in ET.fromstring(capabilities["stdout"])]
    searches = {}
    for plugin in PLUGINS:
        result = capture_process([str(Path(__file__).resolve()), "--worker", plugin, "--query", arguments.query], 20)
        result["parsed"] = inspect_rows(result["stdout"])
        result["status"] = (
            "timeout" if result["timed_out"] else
            "failed" if result["exit_code"] != 0 else
            "results_returned" if result["parsed"]["count"] else
            "zero_results_unverified"
        )
        searches[plugin] = result
    report = {
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "python": sys.version,
        "query": arguments.query,
        "scope": "Search HTTP metadata only; no torrent payload or download service invoked.",
        "runtime": "Copied Python files in a separate interpreter; qBittorrent executable not invoked.",
        "isolation_limit": "Process separation is not a filesystem or security sandbox; only reviewed plugins selected.",
        "capabilities": capabilities,
        "discovered_plugins": discovered,
        "searches": searches,
    }
    (ROOT / "probe.results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({
        "discovered_plugins": discovered,
        "searches": {name: {"status": result["status"], "count": result["parsed"]["count"], "exit_code": result["exit_code"], "seconds": result["elapsed_seconds"]} for name, result in searches.items()},
        "report": str(ROOT / "probe.results.json"),
    }, ensure_ascii=True, indent=2))
    successful = sorted(discovered) == sorted(PLUGINS) and all(
        result["status"] == "results_returned" and not result["parsed"]["malformed_lines"]
        for result in searches.values()
    )
    return 0 if successful else 1


if __name__ == "__main__":
    raise SystemExit(main())
