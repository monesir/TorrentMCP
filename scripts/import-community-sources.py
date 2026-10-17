"""Register explicitly reviewed public community entries; never execute them.

Example: python scripts/import-community-sources.py --entry 041 055
Registration is a deliberate review step, separate from catalog download.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--entry", nargs="+", required=True)
    args = parser.parse_args()
    catalog_path = ROOT / "research/qbittorrent/community-20261017/catalog.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8-sig"))
    manifest_path = ROOT / "resources/sources.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    registered = []
    for identifier in args.entry:
        entry = next(row for row in catalog["entries"] if row["entry_id"] == identifier)
        if entry["visibility"] != "public":
            raise ValueError(f"Private source excluded: {identifier}")
        if entry["fetch_status"] != "downloaded_not_reviewed":
            raise ValueError(f"Source file is unavailable: {identifier}")
        payload = (ROOT / entry["local_path"]).read_bytes()
        if hashlib.sha256(payload).hexdigest() != entry["sha256"]:
            raise ValueError(f"Snapshot changed: {identifier}")
        tree = ast.parse(payload)
        constants = {}
        for node in tree.body:
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    if isinstance(target, ast.Name):
                        try:
                            constants[target.id] = ast.literal_eval(node.value)
                        except (ValueError, TypeError):
                            pass
        classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
        engine = next(node for node in classes if any(isinstance(n, ast.FunctionDef) and n.name == "search" for n in node.body))
        fields = {}
        for node in engine.body:
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    if isinstance(target, ast.Name) and target.id in {"url", "name", "supported_categories"}:
                        fields[target.id] = constants[node.value.id] if isinstance(node.value, ast.Name) else ast.literal_eval(node.value)
            # A few original engines declare literal metadata in __init__.
            # Read only unconditional assignments; never instantiate plugins.
            if isinstance(node, ast.FunctionDef) and node.name == "__init__":
                for assignment in node.body:
                    if isinstance(assignment, ast.Assign):
                        for target in assignment.targets:
                            if (isinstance(target, ast.Attribute) and isinstance(target.value, ast.Name)
                                    and target.value.id == "self" and target.attr in {"url", "name", "supported_categories"}):
                                fields[target.attr] = ast.literal_eval(assignment.value)
        if "url" not in fields or "name" not in fields:
            raise ValueError(f"Metadata requires manual review: {identifier}")
        previous = next((row for row in manifest["sources"] if row.get("community_entry") == identifier), None)
        source_id = previous["id"] if previous else engine.name.lower()
        if not previous and any(row["id"] == source_id for row in manifest["sources"]):
            source_id += f"_community_{identifier}"
        descriptor = dict(id=source_id, class_name=engine.name, name=fields["name"],
                          preserve_site_trailing_slash=fields["url"].endswith("/"),
                          site_url=fields["url"].rstrip("/"), categories=list(fields.get("supported_categories", {"all": ""})),
                          resolves_http=any(isinstance(n, ast.FunctionDef) and n.name == "download_torrent" for n in engine.body),
                          sha256=entry["sha256"], community_entry=identifier)
        (ROOT / "resources/qbit/engines" / f"{source_id}.py").write_bytes(payload)
        if previous:
            manifest["sources"][manifest["sources"].index(previous)] = descriptor
        else:
            manifest["sources"].append(descriptor)
        entry["registered_id"] = source_id
        entry["reviewed_for_native_port"] = True
        entry["native_status"] = "implemented_pending_verification"
        entry["python_retained"] = True
        registered.append(source_id)
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    catalog_path.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(registered=registered)))


if __name__ == "__main__":
    main()
