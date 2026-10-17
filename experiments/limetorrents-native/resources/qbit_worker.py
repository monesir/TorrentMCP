"""Version 1 JSON-lines bridge for selected, trusted qBittorrent plugins.

Each worker hosts one plugin. Process separation supports deadlines and failure
isolation; it does not restrict filesystem or network permissions.
"""

import argparse
import contextlib
import importlib
import io
import json
from pathlib import Path
import socket
import sys
import urllib.parse


class ResultLimit(BaseException):
    """Stop plugins even when they catch ordinary parsing/network exceptions."""


def optional_integer(value):
    try:
        number = int(value)
        return number if 0 <= number <= 2**64 - 1 else None
    except (TypeError, ValueError, OverflowError):
        return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--plugin", required=True)
    parser.add_argument("--operation", choices=("search", "resolve"), required=True)
    parser.add_argument("--value", required=True)
    parser.add_argument("--category", required=True)
    parser.add_argument("--limit", type=int, required=True)
    parser.add_argument("--site-url")
    arguments = parser.parse_args()
    if not arguments.plugin.isidentifier() or not 1 <= arguments.limit <= 1000:
        parser.error("Invalid plugin identifier or result limit")

    output = sys.stdout

    def emit(event, **fields):
        output.write(json.dumps({"protocol": 1, "event": event, **fields}, ensure_ascii=True, allow_nan=False) + "\n")
        output.flush()

    count = 0

    try:
        sys.path.insert(0, str(Path(__file__).resolve().parent / "qbit"))
        socket.setdefaulttimeout(12)
        with contextlib.redirect_stdout(sys.stderr):
            import helpers
            import novaprinter

            original_retrieve = helpers.retrieve_url

            def retrieve_url(*args, **kwargs):
                content = original_retrieve(*args, **kwargs)
                if not content:
                    raise RuntimeError("Source returned an empty HTTP response or the request failed")
                return content

            helpers.retrieve_url = retrieve_url

            def pretty_printer(candidate):
                nonlocal count
                description = candidate.get("desc_link")
                emit("result", candidate={
                    "title": str(candidate["name"]),
                    "uri": str(candidate["link"]),
                    "size_bytes": optional_integer(novaprinter.anySizeToBytes(candidate.get("size", -1))),
                    "seeders": optional_integer(candidate.get("seeds")),
                    "leechers": optional_integer(candidate.get("leech")),
                    "description_url": str(description) if description else None,
                    "published_unix": optional_integer(candidate.get("pub_date")),
                })
                count += 1
                if count >= arguments.limit:
                    raise ResultLimit()

            # Patch before the engine's 'from novaprinter import prettyPrinter'.
            novaprinter.prettyPrinter = pretty_printer
            module = importlib.import_module("engines." + arguments.plugin)
            engine = getattr(module, arguments.plugin)()
            if arguments.site_url:
                engine.url = arguments.site_url.rstrip("/")
            if arguments.operation == "search":
                categories = getattr(engine, "supported_categories", {})
                if categories and arguments.category not in categories:
                    raise ValueError("Plugin does not support the selected category")
                if categories:
                    engine.search(urllib.parse.quote(arguments.value), arguments.category)
                else:
                    engine.search(urllib.parse.quote(arguments.value))
            else:
                if not hasattr(engine, "download_torrent"):
                    raise ValueError("Plugin does not resolve HTTP references")
                captured = io.StringIO()
                with contextlib.redirect_stdout(captured):
                    engine.download_torrent(arguments.value)
                text = captured.getvalue().strip()
                uri = text.split(maxsplit=1)[0] if text else ""
                if not uri.startswith("magnet:"):
                    raise ValueError("This bridge currently supports magnet resolution only")
                emit("resolved", uri=uri)
        emit("complete", limited=False)
        return 0
    except ResultLimit:
        emit("complete", limited=True)
        return 0
    except Exception as error:
        emit("error", message=f"{type(error).__name__}: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

