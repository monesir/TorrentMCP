"""Windows paired benchmark: release CLI + its Python descendants as one job.

Uses Windows Job Object CPU and peak private committed memory counters, plus a
5 ms sampled sum of working sets. The replay HTTP server and this harness are
outside the measured job. Inputs are captured real pages, not synthetic results.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import msvcrt
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.parse

import psutil

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'verification' / 'limetorrents-benchmark'
FIXTURES = OUTPUT / 'fixtures'
K = C.WinDLL('kernel32', use_last_error=True)
SIZE = C.c_size_t


class BasicLimits(C.Structure):
    _fields_ = [('process_time', C.c_int64), ('job_time', C.c_int64), ('flags', W.DWORD),
                ('minimum_ws', SIZE), ('maximum_ws', SIZE), ('process_limit', W.DWORD),
                ('affinity', SIZE), ('priority', W.DWORD), ('scheduling', W.DWORD)]


class IoCounters(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ['reads', 'writes', 'other', 'read_bytes', 'write_bytes', 'other_bytes']]


class ExtendedLimits(C.Structure):
    _fields_ = [('basic', BasicLimits), ('io', IoCounters), ('process_memory', SIZE),
                ('job_memory', SIZE), ('peak_process_memory', SIZE), ('peak_job_memory', SIZE)]


class Accounting(C.Structure):
    _fields_ = [('user', C.c_int64), ('kernel', C.c_int64), ('period_user', C.c_int64),
                ('period_kernel', C.c_int64), ('faults', W.DWORD), ('total_processes', W.DWORD),
                ('active_processes', W.DWORD), ('terminated_processes', W.DWORD)]


class Startup(C.Structure):
    _fields_ = [('cb', W.DWORD), ('reserved', W.LPWSTR), ('desktop', W.LPWSTR), ('title', W.LPWSTR),
                ('x', W.DWORD), ('y', W.DWORD), ('width', W.DWORD), ('height', W.DWORD),
                ('chars_x', W.DWORD), ('chars_y', W.DWORD), ('fill', W.DWORD), ('flags', W.DWORD),
                ('show', W.WORD), ('reserved_bytes', W.WORD), ('reserved_ptr', C.POINTER(C.c_ubyte)),
                ('stdin', W.HANDLE), ('stdout', W.HANDLE), ('stderr', W.HANDLE)]


class ProcessInfo(C.Structure):
    _fields_ = [('process', W.HANDLE), ('thread', W.HANDLE), ('pid', W.DWORD), ('thread_id', W.DWORD)]


for name, args, result in [
    ('CreateJobObjectW', [C.c_void_p, W.LPCWSTR], W.HANDLE),
    ('SetInformationJobObject', [W.HANDLE, C.c_int, C.c_void_p, W.DWORD], W.BOOL),
    ('QueryInformationJobObject', [W.HANDLE, C.c_int, C.c_void_p, W.DWORD, C.c_void_p], W.BOOL),
    ('AssignProcessToJobObject', [W.HANDLE, W.HANDLE], W.BOOL),
    ('CreateProcessW', [W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL, W.DWORD, C.c_void_p, W.LPCWSTR, C.POINTER(Startup), C.POINTER(ProcessInfo)], W.BOOL),
    ('ResumeThread', [W.HANDLE], W.DWORD),
    ('WaitForSingleObject', [W.HANDLE, W.DWORD], W.DWORD),
    ('GetExitCodeProcess', [W.HANDLE, C.POINTER(W.DWORD)], W.BOOL),
    ('TerminateProcess', [W.HANDLE, W.UINT], W.BOOL),
    ('TerminateJobObject', [W.HANDLE, W.UINT], W.BOOL),
    ('CloseHandle', [W.HANDLE], W.BOOL),
]:
    function = getattr(K, name)
    function.argtypes, function.restype = args, result


def checked(result):
    if not result:
        raise C.WinError(C.get_last_error())
    return result


def measure(command, *, timeout=45, parse_failed_report=False, stdout_path=None, stderr_path=None):
    job = checked(K.CreateJobObjectW(None, None))
    limits = ExtendedLimits()
    limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    checked(K.SetInformationJobObject(job, 9, C.byref(limits), C.sizeof(limits)))
    info = ProcessInfo()
    try:
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr, open(os.devnull, 'rb') as stdin:
            handles = [msvcrt.get_osfhandle(file.fileno()) for file in [stdin, stdout, stderr]]
            for handle in handles:
                os.set_handle_inheritable(handle, True)
            startup = Startup()
            startup.cb, startup.flags = C.sizeof(startup), 0x100
            startup.stdin, startup.stdout, startup.stderr = handles
            text = C.create_unicode_buffer(subprocess.list2cmdline([str(arg) for arg in command]))
            started = time.perf_counter()
            checked(K.CreateProcessW(None, text, None, None, True, 0x08000004, None, str(ROOT), C.byref(startup), C.byref(info)))
            checked(K.AssignProcessToJobObject(job, info.process))
            root = psutil.Process(info.pid)
            if K.ResumeThread(info.thread) == 0xFFFFFFFF:
                raise C.WinError(C.get_last_error())
            peak_working_set = 0
            process_names = set()
            while True:
                try:
                    processes = [root, *root.children(recursive=True)]
                    total = 0
                    for process in processes:
                        try:
                            total += process.memory_info().rss
                            process_names.add(process.name())
                        except (psutil.NoSuchProcess, psutil.AccessDenied):
                            pass
                    peak_working_set = max(peak_working_set, total)
                except psutil.NoSuchProcess:
                    pass
                if K.WaitForSingleObject(info.process, 5) == 0:
                    break
                if time.perf_counter() - started > timeout:
                    raise TimeoutError(f'Measured process exceeded {timeout} seconds')
            code = W.DWORD()
            checked(K.GetExitCodeProcess(info.process, C.byref(code)))
            K.CloseHandle(info.thread); info.thread = None
            K.CloseHandle(info.process); info.process = None
            accounting, memory = Accounting(), ExtendedLimits()
            checked(K.QueryInformationJobObject(job, 1, C.byref(accounting), C.sizeof(accounting), None))
            settled = time.perf_counter() + 2
            while accounting.active_processes and time.perf_counter() < settled:
                time.sleep(.002)
                checked(K.QueryInformationJobObject(job, 1, C.byref(accounting), C.sizeof(accounting), None))
            checked(K.QueryInformationJobObject(job, 9, C.byref(memory), C.sizeof(memory), None))
            if accounting.active_processes:
                raise RuntimeError(f'Job did not settle: total={accounting.total_processes}, active={accounting.active_processes}, terminated={accounting.terminated_processes}')
            elapsed = (time.perf_counter() - started) * 1000
            stdout.seek(0); stderr.seek(0)
            output = stdout.read().decode('utf-8')
            diagnostics = stderr.read().decode('utf-8', errors='replace')
            if stdout_path is not None:
                Path(stdout_path).write_text(output, encoding='utf-8')
            if stderr_path is not None:
                Path(stderr_path).write_text(diagnostics, encoding='utf-8')
            record = {
                'wall_ms': elapsed,
                'cpu_ms': (accounting.user + accounting.kernel) / 10000,
                'peak_private_mib': memory.peak_job_memory / 1024**2,
                'sampled_peak_working_set_mib': peak_working_set / 1024**2,
                'process_count': accounting.total_processes,
                'observed_process_names': sorted(process_names),
                'exit_code': code.value,
            }
            if code.value:
                record.update(error=diagnostics[:4000], stdout=output[:4000])
                if not parse_failed_report:
                    return record, None
            try:
                return record, json.loads(output)
            except json.JSONDecodeError as error:
                if not parse_failed_report:
                    raise
                record['report_error'] = str(error)
                return record, None
    finally:
        if info.process:
            K.TerminateProcess(info.process, 1)
        K.TerminateJobObject(job, 1)
        for handle in [info.thread, info.process, job]:
            if handle:
                K.CloseHandle(handle)


class Replay(BaseHTTPRequestHandler):
    count = 0

    def do_GET(self):
        type(self).count += 1
        path = urllib.parse.urlsplit(self.path).path
        pieces = path.strip('/').split('/')
        if len(pieces) == 6 and pieces[0] == 'search' and pieces[4] == 'seeds':
            filename = f'{pieces[3]}-{pieces[5]}.html'
        elif len(pieces) == 5 and pieces[0] == 'search' and pieces[3] == 'seeds':
            filename = f'{pieces[2]}-{pieces[4]}.html'
        else:
            self.send_error(404); return
        target = FIXTURES / filename
        if not target.is_file():
            self.send_error(404); return
        content = target.read_bytes()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, *args):
        pass


def canonical(report):
    values = []
    for result in report['results']:
        origin = result['observations'][0]
        values.append((result['reference']['uri'], result['title'], origin['size_bytes'], origin['seeders'], origin['leechers']))
    return sorted(values)


def summarize(records):
    return {
        backend: {
            key: {'median': statistics.median([row[key] for row in records if row['backend'] == backend]),
                  'min': min(row[key] for row in records if row['backend'] == backend),
                  'max': max(row[key] for row in records if row['backend'] == backend)}
            for key in ['wall_ms', 'cpu_ms', 'peak_private_mib', 'sampled_peak_working_set_mib', 'process_count']
        } for backend in ['python', 'rust']
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--mode', choices=['replay', 'live'], required=True)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--executable', type=Path, default=ROOT / '.cache/limetorrents-experiment-target/release/torrent-search.exe')
    arguments = parser.parse_args()
    executable = arguments.executable.resolve()
    python = ROOT / '.cache/python-3.14.8-embed-amd64/python.exe'
    server = None
    if arguments.mode == 'replay':
        server = ThreadingHTTPServer(('127.0.0.1', 0), Replay)
        threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='limetorrents-replay-', dir=ROOT / '.cache') as directory:
        assets = Path(directory)
        # Resources remain in the repo; only source configuration changes for
        # the explicit local replay endpoint, through the normal mirror setting.
        import shutil
        shutil.copytree(ROOT / 'experiments/limetorrents-native/resources', assets, dirs_exist_ok=True)
        if server:
            manifest = json.loads((assets / 'sources.json').read_text(encoding='utf-8'))
            next(source for source in manifest['sources'] if source['id'] == 'limetorrents')['site_url'] = f'http://127.0.0.1:{server.server_port}'
            (assets / 'sources.json').write_text(json.dumps(manifest), encoding='utf-8')
        queries = ['nirvana in fire', 'ubuntu'] if server else ['nirvana in fire']
        records, reports = [], {}
        for query in queries:
            for trial in range(-1, arguments.trials):
                order = ['python', 'rust'] if trial % 2 == 0 else ['rust', 'python']
                pair = {}
                for backend in order:
                    before = Replay.count
                    command = [executable, '--assets', assets, '--python', python, '--json', '--limetorrents-backend', backend,
                               'search', query, '--source', 'limetorrents', '--limit', '200', '--timeout', '20']
                    measurement, report = measure(command)
                    measurement.update(backend=backend, query=query, trial=trial, request_count=(Replay.count-before) if server else None)
                    if report is None or report['partial']:
                        raise RuntimeError(json.dumps(measurement))
                    pair[backend] = canonical(report)
                    measurement['results'] = len(report['results'])
                    if trial >= 0:
                        records.append(measurement)
                        reports[f'{query}-{backend}'] = report
                if pair['python'] != pair['rust']:
                    (OUTPUT / 'parity-mismatch.json').write_text(json.dumps(pair, ensure_ascii=False, indent=2), encoding='utf-8')
                    raise AssertionError('Source results differ; see parity-mismatch.json')
                print(f'{arguments.mode}: {query}: pair {trial+1}/{arguments.trials}: parity OK', flush=True)
        result = {
            'mode': arguments.mode, 'trials_per_backend_per_query': arguments.trials,
            'binary_sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
            'python_version': '3.14.8', 'records': records,
            'summary': {query: summarize([row for row in records if row['query'] == query]) for query in queries},
            'parity': 'equal URI, title, size, seeders and leechers for every pair',
            'cpu_and_memory_scope': 'Windows Job Object includes Rust root and all Python descendants; server/harness excluded',
            'memory_definition': 'PeakJobMemoryUsed is peak private committed memory; working set is sampled every 5 ms',
        }
        (OUTPUT / f'{arguments.mode}.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        for label, report in reports.items():
            (OUTPUT / f'{arguments.mode}-{label.replace(" ", "-")}.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(result['summary'], indent=2))
    if server:
        server.shutdown(); server.server_close()


if __name__ == '__main__':
    main()
