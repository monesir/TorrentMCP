"""Run the same live query through every registered public plugin backend.

Raw reports, diagnostics and per-source comparisons are retained. Site state,
caches and network latency vary: these observations are not a speed benchmark.
"""
import argparse
from collections import Counter, defaultdict
import csv
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def save(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def title_match(title):
    compact = "".join(c for c in title.casefold() if c.isalnum())
    return any(term in compact for term in ("nirvanainfire", "langyabang", "琅琊榜"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--query", default="nirvana in fire")
    parser.add_argument("--timeout", type=int, default=20)
    parser.add_argument("--limit", type=int, default=20)
    parser.add_argument("--parallel", type=int, default=6)
    args = parser.parse_args()
    bundle = args.bundle.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    executable = bundle / "torrent-search.exe"
    manifest = json.loads((bundle / "resources/sources.json").read_text(encoding="utf-8"))
    sources = manifest["sources"]
    expected = {row["id"] for row in sources}
    if len(sources) != 86 or len(expected) != 86:
        raise ValueError("Expected the verified 86-plugin public snapshot")
    for source in sources:
        plugin = bundle / f"resources/qbit/engines/{source['id']}.py"
        if hashlib.sha256(plugin.read_bytes()).hexdigest() != source["sha256"]:
            raise ValueError(f"Plugin pin changed: {source['id']}")
    state = dict(query=args.query, sources=86, timeout=args.timeout, limit=args.limit,
                 parallel=args.parallel, started_utc=datetime.now(timezone.utc).isoformat(),
                 executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
                 phase="starting", backends={})
    save(output / "state.json", state)
    reports = {}
    for backend in ("rust", "python"):
        # Original plugins can create logs/config/cache. Keep the shipped bundle
        # intact, and give each backend a separate fresh data directory.
        assets = output / backend / "resources"
        shutil.copytree(bundle / "resources", assets)
        home = output / backend / "home"
        (home / "AppData/Roaming").mkdir(parents=True)
        temporary = output / backend / "temp"
        temporary.mkdir()
        env = {**os.environ, "USERPROFILE": str(home), "TEMP": str(temporary), "TMP": str(temporary),
               "TORRENT_SEARCH_CACHE_DIR": str(output / backend / "cache")}
        python = bundle / "runtime/python.exe" if backend == "python" else output / "python-must-not-run.exe"
        command = [str(executable), "--assets", str(assets), "--python", str(python),
                   "--source-backend", backend, "--json", "search", args.query,
                   "--category", "all", "--timeout", str(args.timeout),
                   "--limit", str(args.limit), "--parallel", str(args.parallel)]
        state["phase"] = backend
        save(output / "state.json", state)
        started = time.monotonic()
        with (output / f"{backend}.json").open("wb") as stdout, (output / f"{backend}.stderr.log").open("wb") as stderr:
            result = subprocess.run(command, stdout=stdout, stderr=stderr, env=env,
                                    timeout=(args.timeout + 2) * (len(sources) // args.parallel + 2) + 30)
        report = json.loads((output / f"{backend}.json").read_text(encoding="utf-8-sig"))
        if {source["source_id"] for source in report["sources"]} != expected:
            raise ValueError(f"Missing source reports in {backend}")
        if report["query"] != args.query:
            raise ValueError("Query changed")
        reports[backend] = report
        state["backends"][backend] = dict(exit_code=result.returncode,
            wall_seconds=round(time.monotonic()-started, 3), statuses=dict(Counter(s["status"] for s in report["sources"])),
            merged_results=len(report["results"]))
        save(output / "state.json", state)
        print(json.dumps({backend: state["backends"][backend]}), flush=True)
    by_source = {}
    for backend, report in reports.items():
        values = {row["source_id"]: {**row, "unique_results": 0, "title_matches": 0} for row in report["sources"]}
        seen = defaultdict(set)
        for row in report["results"]:
            ref = row["reference"]
            identity = ref.get("info_hash_v1") or ref["uri"] + json.dumps(ref.get("post_form"), sort_keys=True)
            for observation in row["observations"]:
                identifier = observation["source_id"]
                if identity in seen[identifier]:
                    continue
                seen[identifier].add(identity)
                values[identifier]["unique_results"] += 1
                values[identifier]["title_matches"] += title_match(observation["title"])
        by_source[backend] = values
    rows = []
    for source in sources:
        row = dict(id=source["id"], name=source["name"])
        for backend in ("rust", "python"):
            for key in ("status", "result_count", "unique_results", "title_matches", "elapsed_ms", "limited", "error"):
                row[f"{backend}_{key}"] = by_source[backend][source["id"]][key]
        rows.append(row)
    save(output / "comparison.json", dict(settings=state, sources=rows,
        title_match_note="Heuristic title labels for Nirvana in Fire / Lang Ya Bang / 琅琊榜; not full relevance validation."))
    with (output / "sources.csv").open("w", encoding="utf-8-sig", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    lines = ["# Live search of all public plugins", "", f"Query: `{args.query}`. Category: all. Per-source timeout: {args.timeout}s, limit: {args.limit}, parallel: {args.parallel}.", "",
        "Rust and Python ran sequentially against live websites. Network/cache/site changes affect comparisons; these are observations, not a controlled speed benchmark. Jackett has no local credentials. Completed with zero results does not prove that a site parser works.", "",
        "Title matches are advisory checks for Nirvana in Fire / Lang Ya Bang / 琅琊榜, not full content validation. No torrent payload was downloaded.", "",
        "| Source | Rust status | Results | Title matches | ms | Python status | Results | Title matches | ms |",
        "|---|---|---:|---:|---:|---|---:|---:|---:|"]
    for row in rows:
        lines.append(f"| {row['id']} | {row['rust_status']} | {row['rust_unique_results']} | {row['rust_title_matches']} | {row['rust_elapsed_ms']} | {row['python_status']} | {row['python_unique_results']} | {row['python_title_matches']} | {row['python_elapsed_ms']} |")
    (output / "README.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    state["phase"] = "completed"
    state["completed_utc"] = datetime.now(timezone.utc).isoformat()
    save(output / "state.json", state)
    print(json.dumps(dict(output=str(output), source_pairs=len(rows), completed=True)), flush=True)


if __name__ == "__main__":
    main()
