"""Bounded, measured inspection of saved search candidates using an isolated rqbit service.

No full-file download is requested. Rqbit may fetch extra pieces; counters are preserved.
All session files remain within --work and no downloaded files are deleted.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from urllib.request import Request, urlopen
from urllib.parse import urlencode


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rqbit", type=Path, required=True)
    parser.add_argument("--program", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path)
    parser.add_argument("--candidates", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=False)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    program = str(args.program.resolve(strict=True))
    command = [str(args.rqbit.resolve(strict=True)), "--http-api-listen-addr", f"127.0.0.1:{port}",
               "--disable-dht-persistence", "--disable-upnp-port-forward", "--disable-lsd", "--disable-tcp-listen",
               "--ratelimit-download", "4194304", "--ratelimit-upload", "1024", "--peer-limit", "30",
               "server", "start", "--disable-persistence", str(work)]
    log = (work / "rqbit.log").open("wb")
    service = subprocess.Popen(command, stdout=log, stderr=log, creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
    started = time.perf_counter()
    report = {"rqbit_version": "9.0.1", "service_command": command, "candidates": [], "service_stopped": False}

    def save():
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    def api(path, body=None, timeout=40):
        data = body.encode() if isinstance(body, str) else json.dumps(body).encode() if body is not None else None
        request = Request(base+path, data=data, headers={"Content-Type":"text/plain" if isinstance(body,str) else "application/json"})
        with urlopen(request, timeout=timeout) as response:
            raw = response.read(8*1024*1024)
            return json.loads(raw) if raw else None

    try:
        deadline=time.perf_counter()+10
        while True:
            try: api("/", timeout=1); break
            except Exception:
                if service.poll() is not None or time.perf_counter()>deadline: raise
                time.sleep(0.1)
        report["startup_ms"]=round((time.perf_counter()-started)*1000)
        for index,candidate in enumerate(load(args.candidates)):
            item={"index":index,"title":candidate["title"],"resolve_ms":candidate["resolve_ms"]}
            report["candidates"].append(item)
            torrent_id=None
            phase=time.perf_counter()
            try:
                print(json.dumps({"phase":"metadata","index":index}),flush=True)
                metadata=api("/torrents?is_url=true&list_only=true&timeout_ms=30000",candidate["reference"]["uri"],timeout=35)
                item["metadata_ms"]=round((time.perf_counter()-phase)*1000)
                item["metadata"]=metadata
                files=metadata["details"]["files"]
                videos=[(i,f) for i,f in enumerate(files) if f["name"].lower().endswith((".mkv",".mp4",".avi",".m2ts")) and not f.get("attributes",{}).get("symlink")]
                if not videos: raise RuntimeError("No video file in metadata")
                file_id,file=max(videos,key=lambda pair:pair[1]["length"])
                item["selected_file_id"]=file_id
                # Mark new placeholders sparse BEFORE rqbit calls set_len on Windows.
                # Otherwise creating a 27 GiB logical file can allocate the full size first.
                output_folder=work/f"candidate-{index}"
                output_folder.mkdir()
                for entry in files:
                    if entry.get("attributes",{}).get("symlink"): raise RuntimeError("Symlinks are not inspection inputs")
                    components=entry["components"]
                    if any(component in ("", ".", "..") or "/" in component or "\\" in component or ":" in component for component in components):
                        raise RuntimeError("Unsafe torrent file path")
                    path=output_folder.joinpath(*components)
                    if not path.resolve().is_relative_to(output_folder.resolve()): raise RuntimeError("File path escapes output directory")
                    path.parent.mkdir(parents=True,exist_ok=True)
                    with path.open("xb"): pass
                    if os.name=="nt":
                        subprocess.run([str(Path(os.environ["SystemRoot"])/"System32/fsutil.exe"),"sparse","setflag",str(path)],check=True,capture_output=True)
                # The default all-files downloader is disabled as soon as initialization permits.
                add_start=time.perf_counter()
                added=api("/torrents?"+urlencode({"is_url":"true","only_files":str(file_id),"timeout_ms":30000,
                    "output_folder":str(output_folder),"overwrite":"true"}),candidate["reference"]["uri"],timeout=35)
                torrent_id=added["id"]
                deadline=time.perf_counter()+15
                while True:
                    try:
                        api(f"/torrents/{torrent_id}/update_only_files",{"only_files":[]},timeout=3)
                        break
                    except Exception:
                        if time.perf_counter()>deadline: raise
                        time.sleep(0.1)
                item["add_and_initialize_ms"]=round((time.perf_counter()-add_start)*1000)
                item["stats_before"]=api(f"/torrents/{torrent_id}/stats/v1")
                request={"schema_version":1,"operation":"inspect_media","media":{
                    "input":{"kind":"download_service","url":f"{base}/torrents/{torrent_id}/stream/{file_id}","size_bytes":file["length"]},
                    "limits":{"timeout_seconds":60,"max_read_bytes":16777216},
                    "selection":{"audio_languages":["en"],"subtitle_languages":["ar","en"]}}}
                print(json.dumps({"phase":"probe","index":index,"file":file["name"]}),flush=True)
                probe_start=time.perf_counter()
                probe_command=[program]
                if args.ffprobe: probe_command += ["--ffprobe",str(args.ffprobe.resolve(strict=True))]
                probe_command += ["agent"]
                completed=subprocess.run(probe_command,input=json.dumps(request).encode(),capture_output=True,timeout=70,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
                item["probe_wall_ms"]=round((time.perf_counter()-probe_start)*1000)
                item["probe_exit"]=completed.returncode
                item["probe_stderr"]=completed.stderr.decode("utf-8","replace")
                item["inspection"]=json.loads(completed.stdout)
                item["stats_after"]=api(f"/torrents/{torrent_id}/stats/v1")
            except Exception as error:
                item["failure"]=str(error)
                item["failed_phase_elapsed_ms"]=round((time.perf_counter()-phase)*1000)
            finally:
                if torrent_id is not None:
                    try: api(f"/torrents/{torrent_id}/pause",{})
                    except Exception as error: item["pause_failure"]=str(error)
                    try: api(f"/torrents/{torrent_id}/forget",{})
                    except Exception as error: item["forget_failure"]=str(error)
                save()
            print(json.dumps({"phase":"candidate_finished","index":index,"status":item.get("inspection",{}).get("status"),"failure":item.get("failure")}),flush=True)
    finally:
        service.terminate()
        try: service.wait(timeout=5)
        except subprocess.TimeoutExpired: service.kill(); service.wait(timeout=5)
        log.close()
        report["service_stopped"]=True
        report["total_wall_ms"]=round((time.perf_counter()-started)*1000)
        save()


if __name__=="__main__":
    main()
