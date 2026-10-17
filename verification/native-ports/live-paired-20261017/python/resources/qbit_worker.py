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


def reference_uri(value):
    uri = str(value)
    # Several original community plugins quote an entire HTTP URL, including
    # its scheme separator. qBittorrent unquotes it before download_torrent.
    if uri.lower().startswith(("http%3a//", "https%3a//")):
        uri = urllib.parse.unquote(uri)
    return uri


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--plugin", required=True)
    parser.add_argument("--class-name")
    parser.add_argument("--operation", choices=("search", "resolve"), required=True)
    parser.add_argument("--value", required=True)
    parser.add_argument("--category", required=True)
    parser.add_argument("--limit", type=int, required=True)
    parser.add_argument("--site-url")
    parser.add_argument("--preserve-site-trailing-slash", action="store_true")
    arguments = parser.parse_args()
    if (not arguments.plugin.isidentifier()
            or (arguments.class_name and not arguments.class_name.isidentifier())
            or not 1 <= arguments.limit <= 1000):
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

            # Older community engines import the public `headers` mapping,
            # which current qBittorrent renamed to `_headers`.
            if not hasattr(helpers, "headers") and hasattr(helpers, "_headers"):
                helpers.headers = dict(helpers._headers)

            original_retrieve = helpers.retrieve_url

            def retrieve_url(*args, **kwargs):
                content = original_retrieve(*args, **kwargs)
                if not content:
                    raise RuntimeError("Source returned an empty HTTP response or the request failed")
                return content

            helpers.retrieve_url = retrieve_url

            # Search prepares a download reference. Leave .torrent metadata
            # fetching to the independent download service, without temp files.
            def download_reference(url, *args, **kwargs):
                parsed = urllib.parse.urlsplit(url)
                if parsed.scheme not in ("http", "https") or not parsed.netloc:
                    raise ValueError("Invalid HTTP torrent download URL")
                return f"{url} {url}"

            helpers.download_file = download_reference

            def pretty_printer(candidate):
                nonlocal count
                description = candidate.get("desc_link")
                uri = reference_uri(candidate["link"])
                post_form = None
                if arguments.plugin == "zooqle" and not uri.startswith(("magnet:","http://","https://")):
                    post_form = {"hid": uri}
                    uri = engine.url.rstrip("/") + "/torfile/#hid=" + urllib.parse.quote_plus(uri)
                emit("result", candidate={
                    "title": str(candidate["name"]),
                    "uri": uri,
                    "post_form": post_form,
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
            engine = getattr(module, arguments.class_name or arguments.plugin)()
            if arguments.site_url:
                trailing_slash = arguments.preserve_site_trailing_slash and str(getattr(engine, "url", "")).endswith("/")
                engine.url = arguments.site_url.rstrip("/") + ("/" if trailing_slash else "")
                type(engine).url = engine.url
            if arguments.operation == "search":
                categories = getattr(engine, "supported_categories", {})
                if categories and arguments.category not in categories:
                    raise ValueError("Plugin does not support the selected category")
                if categories:
                    engine.search(urllib.parse.quote(arguments.value), arguments.category)
                else:
                    engine.search(urllib.parse.quote(arguments.value))
            else:
                parsed = urllib.parse.urlsplit(arguments.value)
                if arguments.plugin in {"cpasbien","torrent9"}:
                    if parsed.hostname != urllib.parse.urlsplit(engine.real_url).hostname:
                        raise ValueError("Reference must belong to the source's current public domain")
                if ((arguments.plugin == "smallgames" and parsed.path == "/getTorrent.php")
                        or (arguments.plugin == "rutor" and parsed.path.startswith("/download/"))):
                    emit("resolved", uri=arguments.value)
                    emit("complete", limited=False)
                    return 0
                if arguments.plugin == "zooqle":
                    parsed = urllib.parse.urlsplit(arguments.value)
                    hid = urllib.parse.parse_qs(parsed.fragment).get("hid", [None])[0]
                    if not hid or parsed.path != "/torfile/":
                        raise ValueError("Zooqle resolution requires the POST reference returned by search")
                    emit("resolved", uri=arguments.value, post_form={"hid":hid})
                    emit("complete", limited=False)
                    return 0
                if not hasattr(engine, "download_torrent"):
                    raise ValueError("Plugin does not resolve HTTP references")
                captured = io.StringIO()
                with contextlib.redirect_stdout(captured):
                    engine.download_torrent(arguments.value)
                text = captured.getvalue().strip()
                uri = text.split(maxsplit=1)[0] if text else ""
                if not uri.startswith(("magnet:", "http://", "https://")):
                    raise ValueError("Plugin did not return a magnet or HTTP download reference")
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

