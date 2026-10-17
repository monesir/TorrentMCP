"""Compare CLI inspection of a real local file with the downloader range contract.

python scripts/verify-media-inspection.py --program dist/.../torrent-search.exe \
    --media path/to/existing.mkv --output verification/media-smoke.json
Use --ffprobe path/to/ffprobe.exe only for development builds.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import subprocess
import threading


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--program", type=Path, required=True)
    parser.add_argument("--media", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path)
    args = parser.parse_args()
    media = args.media.resolve(strict=True)
    size = media.stat().st_size
    reads = []

    class RangeHandler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            try:
                raw = self.headers["Range"]
                start, end = map(int, raw.removeprefix("bytes=").split("-"))
                if self.path != "/file" or not 0 <= start <= end < size:
                    raise ValueError("Invalid range")
            except (ValueError, TypeError):
                self.send_error(416)
                return
            count = end - start + 1
            with media.open("rb") as source:
                source.seek(start)
                payload = source.read(count)
            reads.append({"start": start, "end": end, "bytes": len(payload)})
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.send_header("Content-Length", str(count))
            self.send_header("Connection", "close")
            self.end_headers()
            try:
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError):
                pass

    command = [str(args.program.resolve(strict=True))]
    if args.ffprobe:
        command += ["--ffprobe", str(args.ffprobe.resolve(strict=True))]
    command += ["agent"]
    environment = os.environ.copy()
    if os.name == "nt":
        environment["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
    environment["PYTHONHOME"] = "C:/nonexistent/runtime"
    environment["PYTHONPATH"] = "C:/nonexistent/modules"

    def inspect(source):
        request = {"schema_version": 1, "operation": "inspect_media", "media": {
            "input": source,
            "limits": {"timeout_seconds": 30, "max_read_bytes": 16777216},
            "selection": {"audio_languages": ["ja"], "subtitle_languages": ["en"]},
        }}
        process = subprocess.run(command, input=json.dumps(request).encode("utf-8"),
                                 capture_output=True, timeout=40, env=environment)
        response = json.loads(process.stdout)
        if process.returncode != 0 or process.stderr or response["status"] != "completed":
            raise RuntimeError({"exit": process.returncode, "stderr": process.stderr.decode("utf-8", "replace"), "response": response})
        return response

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), RangeHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        local = inspect({"kind": "local_file", "path": str(media)})
        remote = inspect({"kind": "download_service", "url": f"http://127.0.0.1:{server.server_port}/file", "size_bytes": size})
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    if local["result"]["streams"] != remote["result"]["streams"]:
        raise RuntimeError("Local and range-backed stream metadata differ")
    if not 0 < sum(item["bytes"] for item in reads) < size:
        raise RuntimeError("This sample did not demonstrate partial file reading")
    report = {"program_sha256": hashlib.sha256(args.program.read_bytes()).hexdigest(),
              "media_sha256": hashlib.sha256(media.read_bytes()).hexdigest(),
              "media_size_bytes": size, "system_tools_excluded_from_child_path": os.name == "nt",
              "local": local, "download_service": remote, "downloader_reads": reads,
              "matching_streams": True}
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"output": str(args.output), "streams": len(local["result"]["streams"]),
                      "file_size": size, "downloader_bytes": sum(item["bytes"] for item in reads),
                      "matching_streams": True}))


if __name__ == "__main__":
    main()
