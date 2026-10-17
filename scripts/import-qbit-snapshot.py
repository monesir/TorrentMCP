"""Import a reviewed, local qBittorrent source snapshot without executing it.

The source commit and the original files remain in research/. This does not
download or enable unreviewed community scripts. Run from any directory.
"""
import ast
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT = ROOT / "research/qbittorrent/upstream-20261017"


def main():
    commit = (SNAPSHOT / "commit.txt").read_text().strip()
    manifest_path = ROOT / "resources/sources.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    records, sources = [], []
    for path in sorted(SNAPSHOT.glob("*.py")):
        payload = path.read_bytes()
        tree = ast.parse(payload)
        engine = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == path.stem)
        fields = {}
        for node in engine.body:
            if not isinstance(node, ast.Assign):
                continue
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id in {"name", "url", "supported_categories"}:
                    try:
                        fields[target.id] = ast.literal_eval(node.value)
                    except (ValueError, TypeError):
                        pass
        # Jackett computes its URL from a separate, local secret configuration.
        url = fields.get("url", "http://127.0.0.1:9117" if path.stem == "jackett" else None)
        if not url or "name" not in fields or "supported_categories" not in fields:
            raise ValueError(f"Review metadata manually: {path}")
        digest = hashlib.sha256(payload).hexdigest()
        descriptor = dict(id=path.stem, name=fields["name"], site_url=url.rstrip("/"),
                          categories=list(fields["supported_categories"]),
                          resolves_http=any(isinstance(n, ast.FunctionDef) and n.name == "download_torrent" for n in engine.body),
                          sha256=digest)
        target = ROOT / "resources/qbit/engines" / path.name
        target.write_bytes(payload)
        sources.append(descriptor)
        records.append(dict(**descriptor, upstream_commit=commit,
                            upstream_url=f"https://github.com/qbittorrent/search-plugins/blob/{commit}/nova3/engines/{path.name}",
                            version=next((line.split(":", 1)[1].strip() for line in payload.decode().splitlines() if line.startswith("# VERSION:")), None),
                            rust_status="implemented_pending_verification",
                            python_retained=True))
    manifest["sources"] = sources + [row for row in manifest["sources"] if row["id"] not in {item["id"] for item in sources}]
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (SNAPSHOT / "manifest.json").write_text(json.dumps(dict(upstream_commit=commit, sources=records), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"commit": commit, "sources": len(sources), "python_retained": True}))


if __name__ == "__main__":
    main()
