"""Controlled end-to-end check: real rqbit seeder, local tracker, CLI, and ffprobe.

No third-party media is used. All generated files, requests and responses are retained.
Usage: python scripts/verify-torrent-workflow.py --program ... --rqbit ... --ffmpeg ... --ffprobe ... --output NEW_DIRECTORY
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import socket
import struct
import subprocess
import threading
import time
from urllib.parse import urlencode, urlsplit, parse_qs
from urllib.request import Request, urlopen


def bencode(value):
    if isinstance(value, int):
        return b"i" + str(value).encode() + b"e"
    if isinstance(value, str):
        value = value.encode()
    if isinstance(value, bytes):
        return str(len(value)).encode() + b":" + value
    if isinstance(value, list):
        return b"l" + b"".join(bencode(v) for v in value) + b"e"
    return b"d" + b"".join(bencode(k) + bencode(value[k]) for k in sorted(value)) + b"e"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser()
    for arg in ("program", "rqbit", "ffmpeg", "ffprobe", "output"):
        parser.add_argument("--" + arg, type=Path, required=True)
    parser.add_argument("--bundled-runtime", action="store_true", help="Exercise the executable's default bundled runtime discovery")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    seed = output / "seed"
    seed.mkdir()
    for lang in ("en", "ar"):
        (output / (lang + ".srt")).write_text("1\n00:00:00,000 --> 00:00:01,500\n" + ("Sample subtitle" if lang == "en" else "ترجمة تجريبية") + "\n", encoding="utf-8")
    command = [str(args.ffmpeg.resolve()), "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=24:duration=2",
               "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-f", "lavfi", "-i", "sine=frequency=880:sample_rate=48000:duration=2",
               "-i", str(output / "en.srt"), "-i", str(output / "ar.srt"), "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s", "-map", "4:s",
               "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-c:s", "srt", "-metadata:s:a:0", "language=eng", "-metadata:s:a:1", "language=ara",
               "-metadata:s:s:0", "language=eng", "-metadata:s:s:1", "language=ara", str(seed / "E01.mkv")]
    subprocess.run(command, check=True, timeout=30, capture_output=True)
    subprocess.run([str(args.ffmpeg.resolve()), "-hide_banner", "-loglevel", "error", "-i", str(seed / "E01.mkv"), "-map", "0", "-c", "copy", "-metadata", "comment=" + "x"*4096, str(seed / "E02.mkv")], check=True, timeout=30, capture_output=True)
    subprocess.run([str(args.ffmpeg.resolve()), "-hide_banner", "-loglevel", "error", "-i", str(seed / "E01.mkv"), "-map", "0:v", "-map", "0:a:0", "-map", "0:s:0", "-c", "copy", str(seed / "E03.mkv")], check=True, timeout=30, capture_output=True)
    (seed / "readme.txt").write_text("Controlled fixture only", encoding="utf-8")
    names = ["E01.mkv", "E02.mkv", "E03.mkv", "readme.txt"]
    content = b"".join((seed / n).read_bytes() for n in names)
    info = {"name": "Fixture", "piece length": 65536, "pieces": b"".join(hashlib.sha1(content[i:i+65536]).digest() for i in range(0, len(content), 65536)),
            "private": 0, "files": [{"length": (seed / n).stat().st_size, "path": [n]} for n in names]}
    info_hash = hashlib.sha1(bencode(info)).hexdigest()
    peer_port, api_port = port(), port()
    requests = []
    torrent_bytes = b""

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            parsed = urlsplit(self.path)
            requests.append({"method": "GET", "path": parsed.path})
            if parsed.path == "/announce":
                response = bencode({"interval": 1, "complete": 1, "incomplete": 0, "peers": socket.inet_aton("127.0.0.1") + struct.pack("!H", peer_port)})
            elif parsed.path == "/fixture.torrent":
                response = torrent_bytes
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            requests.append({"method": "POST", "path": self.path, "body": body.decode()})
            if self.path != "/fixture.torrent" or parse_qs(body.decode()) != {"id": ["fixture"]}:
                self.send_error(400)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(torrent_bytes)))
            self.end_headers()
            self.wfile.write(torrent_bytes)

    http = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    http.daemon_threads = True
    origin = "http://127.0.0.1:" + str(http.server_port)
    torrent_bytes = bencode({"announce": origin + "/announce", "info": info})
    (output / "fixture.torrent").write_bytes(torrent_bytes)
    threading.Thread(target=http.serve_forever, daemon=True).start()
    log = (output / "seeder.log").open("wb")
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    seeder = subprocess.Popen([str(args.rqbit.resolve()), "-v", "debug", "--http-api-listen-addr", f"127.0.0.1:{api_port}", "--listen-port", str(peer_port),
                              "--disable-dht", "--disable-upnp-port-forward", "--disable-lsd", "server", "start", "--disable-persistence", str(seed)], stdout=log, stderr=log, creationflags=flags)
    api = f"http://127.0.0.1:{api_port}"
    summary = {"info_hash_v1": info_hash, "seeder_stopped": False, "checks": []}

    def service_request(path, data=None):
        with urlopen(Request(api + path, data=data, headers={"Content-Type": "application/x-bittorrent"}), timeout=10) as response:
            return json.load(response)

    def agent(name, request):
        (output / (name + "-request.json")).write_text(json.dumps(request, ensure_ascii=False, indent=2), encoding="utf-8")
        started = time.perf_counter()
        command = [str(args.program.resolve()), "--assets", "missing-assets", "--python", "missing-python", "--torrent-cache", str(output / "sessions")]
        if not args.bundled_runtime:
            command += ["--rqbit", str(args.rqbit.resolve()), "--ffprobe", str(args.ffprobe.resolve())]
        command += ["agent"]
        result = subprocess.run(command, input=json.dumps(request).encode(), capture_output=True, timeout=85, creationflags=flags)
        (output / (name + ".json")).write_bytes(result.stdout)
        (output / (name + ".stderr.txt")).write_bytes(result.stderr)
        envelope = json.loads(result.stdout)
        assert not result.stderr, result.stderr
        if envelope["result"] and "service" in envelope["result"]:
            assert envelope["result"]["service"]["stopped"] is True, envelope
        summary["checks"].append({"name": name, "exit": result.returncode, "status": envelope["status"], "elapsed_ms": round((time.perf_counter()-started)*1000), "error": envelope.get("error")})
        return result.returncode, envelope

    try:
        deadline = time.monotonic() + 10
        while True:
            try:
                service_request("/")
                break
            except Exception:
                if seeder.poll() is not None or time.monotonic() > deadline:
                    raise
                time.sleep(.05)
        added = service_request("/torrents?" + urlencode({"is_url": "false", "overwrite": "true", "output_folder": str(seed)}), torrent_bytes)
        torrent_id = added["id"]
        deadline = time.monotonic() + 15
        while True:
            stats = service_request(f"/torrents/{torrent_id}/stats/v1")
            if stats.get("finished"):
                break
            assert time.monotonic() < deadline, stats
            time.sleep(.1)
        reference = {"kind": "http", "uri": origin + "/fixture.torrent", "source_id": "controlled-fixture", "post_form": {"id": "fixture"}}
        # A search result reference is passed intact; POST references require no search runtime.
        search = {"title": "Controlled fixture", "reference": reference, "observations": []}
        (output / "selected-search-result.json").write_text(json.dumps(search, indent=2), encoding="utf-8")
        code, response = agent("files", {"schema_version": 1, "operation": "torrent_files", "reference": search["reference"], "limits": {"timeout_seconds": 45}})
        assert code == 0, response
        catalog = response["result"]["torrent"]
        assert catalog["info_hash_v1"] == info_hash, catalog
        assert len(catalog["files"]) == 4
        assert catalog["source_reference"] == reference
        assert not list(Path(response["result"]["service"]["working_directory"]).rglob("*.mkv")), "Metadata listing must not create media files"
        direct = {"kind": "http", "uri": origin + "/fixture.torrent", "source_id": "controlled-fixture"}
        code, direct_response = agent("direct-http-files", {"schema_version": 1, "operation": "torrent_files", "reference": direct, "resolve_http": False})
        assert code == 0 and direct_response["result"]["torrent"]["info_hash_v1"] == info_hash, direct_response
        first = next(f for f in catalog["files"] if f["path"].endswith("E01.mkv"))
        assert first["size_bytes"] < max(f["size_bytes"] for f in catalog["files"]), "Fixture must catch largest-file selection"
        request = {"schema_version": 1, "operation": "inspect_torrent_file", "torrent": catalog, "file_id": first["id"], "limits": {"timeout_seconds": 60},
                   "inspection_limits": {"timeout_seconds": 45, "max_read_bytes": 16777216}, "selection": {"audio_languages": ["ar"], "subtitle_languages": ["ar", "en"]}}
        analysis = {"constraints": {"min_video_width": 160, "min_video_height": 90, "audio_languages": ["en"], "subtitle_languages": ["ar"], "max_file_size_bytes": 1048576},
                    "preferences": {"rank_by": ["smaller_file"]}}
        request["analysis"] = analysis
        code, response = agent("inspection", request)
        assert code == 0, response
        assert response["result"]["file"] == first
        assert response["result"]["torrent"]["info_hash_v1"] == info_hash
        streams = response["result"]["media"]["streams"]
        assert len(streams) == 5, streams
        assert {s["language"] for s in streams if s["kind"] == "audio"} == {"en", "ar"}
        assert {s["language"] for s in streams if s["kind"] == "subtitle"} == {"en", "ar"}
        assert not response["result"]["media"]["selection"]["missing_audio_languages"]
        assert response["result"]["assessment"]["eligibility"] == "eligible"
        inspected = [{"id": "complete-small", "inspection": response}]
        for name, candidate_id in [("E02.mkv", "complete-large"), ("E03.mkv", "missing-arabic")]:
            other = json.loads(json.dumps(request))
            other["file_id"] = next(f["id"] for f in catalog["files"] if f["path"].endswith(name))
            code, other_response = agent("inspection-" + candidate_id, other)
            assert code == 0, other_response
            assert other_response["result"]["assessment"]["eligibility"] == ("excluded" if candidate_id == "missing-arabic" else "eligible"), other_response
            inspected.append({"id": candidate_id, "inspection": other_response})
        bad = json.loads(json.dumps(request))
        bad["torrent"]["files"][0]["size_bytes"] += 1
        code, response = agent("changed-catalog", bad)
        assert code == 1 and response["error"]["code"] == "torrent_catalog_changed", response
        bad = json.loads(json.dumps(request))
        bad["file_id"] = 99
        code, response = agent("invalid-selection", bad)
        assert code == 1 and response["error"]["code"] == "invalid_request", response
        magnet = {"kind": "magnet", "uri": f"magnet:?xt=urn:btih:{info_hash}&tr=" + origin.replace(":", "%3A").replace("/", "%2F") + "%2Fannounce", "info_hash_v1": info_hash}
        code, response = agent("magnet-files", {"schema_version": 1, "operation": "torrent_files", "reference": magnet, "limits": {"timeout_seconds": 45}})
        assert code == 0 and response["result"]["torrent"]["info_hash_v1"] == info_hash, response
        missing = {"kind": "magnet", "uri": "magnet:?xt=urn:btih:" + "0"*40, "info_hash_v1": "0"*40}
        code, response = agent("timeout", {"schema_version": 1, "operation": "torrent_files", "reference": missing, "limits": {"timeout_seconds": 1}})
        assert code == 1 and response["error"]["code"] == "torrent_service_timeout", response
        inspected.append({"id": "unavailable", "inspection": response})
        code, comparison = agent("comparison", {"schema_version": 1, "operation": "compare_media", "analysis": analysis, "candidates": inspected})
        assert code == 3, comparison
        assert comparison["result"]["summary"] == {"total": 4, "eligible": 2, "uncertain": 1, "excluded": 1}, comparison
        assert [r["id"] for r in comparison["result"]["results"]] == ["complete-small", "complete-large", "unavailable", "missing-arabic"], comparison
        summary["comparison"] = comparison["result"]["summary"]
        summary["success"] = True
    finally:
        seeder.terminate()
        try:
            seeder.wait(timeout=5)
        except subprocess.TimeoutExpired:
            seeder.kill()
            seeder.wait(timeout=5)
        log.close()
        http.shutdown()
        http.server_close()
        summary["seeder_stopped"] = seeder.poll() is not None
        (output / "http-requests.json").write_text(json.dumps(requests, indent=2), encoding="utf-8")
        (output / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
