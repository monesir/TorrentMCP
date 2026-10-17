"""Serial, resumable Windows live benchmark of the shipped public adapters.

The same CLI hosts both backends. Windows Job Objects account for the CLI and
all Python descendants; the harness stays outside the measured job. No source
code, source URLs, VPN settings, or original plugin files are modified.
"""
import argparse
from collections import Counter
from contextlib import contextmanager
import csv
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
METRICS = ('wall_ms', 'cpu_ms', 'peak_private_mib', 'sampled_peak_working_set_mib')


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    temporary.replace(path)


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def utc():
    return datetime.now(timezone.utc).isoformat()


@contextmanager
def environment(values):
    previous = {key: os.environ.get(key) for key in values}
    os.environ.update({key: str(value) for key, value in values.items()})
    try:
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def report_details(report, source):
    if report is None:
        return dict(status='measurement_failed', result_count=0, identities=[],
                    error='No parseable CLI report', configuration_diagnostic=False)
    if len(report['sources']) != 1 or report['sources'][0]['source_id'] != source:
        raise ValueError('The measured CLI must report exactly the selected source')
    row = report['sources'][0]
    identities = set()
    diagnostic = False
    for result in report['results']:
        # Jackett's original plugin emits its configuration error as a result.
        # Retain its raw report but never count that row as a found torrent.
        if source == 'jackett' and result['title'].startswith('Jackett:'):
            diagnostic = True
            continue
        ref = result['reference']
        identities.add(ref.get('info_hash_v1') or ref['uri'] + json.dumps(ref.get('post_form'), sort_keys=True))
    return dict(status=row['status'], result_count=row['result_count'],
                unique_results=len(identities), identities=sorted(identities),
                error=row.get('error'), source_elapsed_ms=row['elapsed_ms'],
                configuration_diagnostic=diagnostic)


def comparable_pair(rust, python):
    return all(row['status'] == 'completed' and row['identities']
               and not row['configuration_diagnostic'] for row in (rust, python)) \
        and rust['identities'] == python['identities']


def summarize(output, settings, sources):
    records = [read(path) for path in sorted((output / 'records').glob('*.json'))]
    indexed = {(row['source'], row['trial'], row['backend']): row for row in records}
    rows = []
    for source in sources:
        identifier = source['id']
        row = dict(source=identifier, name=source['name'])
        for backend in ('rust', 'python'):
            selected = [r for r in records if r['source'] == identifier and r['backend'] == backend]
            row[f'{backend}_trials'] = len(selected)
            row[f'{backend}_statuses'] = dict(Counter(r['status'] for r in selected))
            for metric in (*METRICS, 'unique_results', 'process_count'):
                values = [r[metric] for r in selected if metric in r]
                row[f'{backend}_{metric}_median'] = statistics.median(values) if values else None
            row[f'{backend}_errors'] = sorted({r['error'] for r in selected if r.get('error')})
        pairs = []
        for trial in range(settings['trials']):
            rust = indexed.get((identifier, trial, 'rust'))
            python = indexed.get((identifier, trial, 'python'))
            if rust and python and comparable_pair(rust, python):
                pairs.append((rust, python))
        row['comparable_trials'] = len(pairs)
        row['python_over_rust_wall_ratio'] = statistics.median(p['wall_ms'] / r['wall_ms'] for r, p in pairs) if pairs else None
        row['python_over_rust_cpu_ratio'] = statistics.median(p['cpu_ms'] / r['cpu_ms'] for r, p in pairs if r['cpu_ms'] > 0) if pairs and any(r['cpu_ms'] > 0 for r, _ in pairs) else None
        row['rust_private_memory_reduction_pct'] = statistics.median(100 * (1 - r['peak_private_mib'] / p['peak_private_mib']) for r, p in pairs if p['peak_private_mib'] > 0) if pairs and any(p['peak_private_mib'] > 0 for _, p in pairs) else None
        rows.append(row)
    save(output / 'comparison.json', dict(settings=settings, completed_measurements=len(records),
         measurement_definition='Fresh CLI processes, serial runs; job CPU and peak private committed memory include all descendants. Working-set sums are sampled and may count shared pages more than once. Network and OS caches are uncontrolled.',
         comparable_definition='Both backends completed with positive, identical result identity sets in the same trial. Timing ratios are not computed for failed, empty, configuration-error or different-result pairs.', sources=rows))
    with (output / 'sources.csv').open('w', encoding='utf-8-sig', newline='') as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        for row in rows:
            writer.writerow({key: json.dumps(value, ensure_ascii=False) if isinstance(value, (dict, list)) else value for key, value in row.items()})
    return rows, len(records)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--query', default='nirvana in fire')
    parser.add_argument('--timeout', type=int, default=20)
    parser.add_argument('--limit', type=int, default=20)
    parser.add_argument('--source', nargs='+', help='Optional selection for harness validation')
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    if os.name != 'nt' or args.trials < 1 or not 1 <= args.timeout <= 120 or not 1 <= args.limit <= 1000:
        parser.error('Requires Windows, trials >= 1, timeout 1..120, limit 1..1000')
    bundle = args.bundle.resolve(strict=True)
    output = args.output.resolve()
    executable, python = bundle / 'torrent-search.exe', bundle / 'runtime/python.exe'
    manifest = read(bundle / 'resources/sources.json')
    assert len(manifest['sources']) == 86 and len({s['id'] for s in manifest['sources']}) == 86
    for entry in manifest['sources']:
        if sha256(bundle / f"resources/qbit/engines/{entry['id']}.py") != entry['sha256']:
            raise ValueError(f"Plugin pin changed: {entry['id']}")
    for entry in manifest['common_files']:
        if sha256(bundle / 'resources' / entry['path']) != entry['sha256']:
            raise ValueError(f"Common file pin changed: {entry['path']}")
    sources = [s for s in manifest['sources'] if not args.source or s['id'] in args.source]
    if not sources or args.source and set(args.source) != {s['id'] for s in sources}:
        raise ValueError('Unknown or empty source selection')
    settings = dict(bundle=str(bundle), executable_sha256=sha256(executable),
                    python_sha256=sha256(python), manifest_sha256=sha256(bundle / 'resources/sources.json'),
                    query=args.query, category='all', limit=args.limit, timeout=args.timeout,
                    trials=args.trials, source_ids=[s['id'] for s in sources],
                    parallel_measured_processes=1, order_seed=20261018)
    if output.exists():
        if not args.resume or read(output / 'settings.json') != settings:
            raise ValueError('Output exists: resume only with exactly the same settings and bundle')
    else:
        output.mkdir(parents=True)
        save(output / 'settings.json', settings)
        save(output / 'host.json', dict(platform=platform.platform(), python=sys.version,
             processor=platform.processor(), logical_cpus=os.cpu_count(),
             started_utc=utc(), note='No VPN, power-plan or background-process settings were changed.'))
    spec = importlib.util.spec_from_file_location('limetorrents_measurement', ROOT / 'scripts/benchmark-limetorrents.py')
    meter = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(meter)
    expected = len(sources) * args.trials * 2
    (output / 'records').mkdir(exist_ok=True)
    completed = len(list((output / 'records').glob('*.json')))
    state = dict(phase='running', pid=os.getpid(), expected_measurements=expected,
                 completed_measurements=completed, updated_utc=utc())
    save(output / 'state.json', state)
    for trial in range(args.trials):
        ordered = sources.copy()
        random.Random(settings['order_seed'] + trial).shuffle(ordered)
        for index, source in enumerate(ordered):
            identifier = source['id']
            order = ('rust', 'python') if (trial + index) % 2 == 0 else ('python', 'rust')
            for backend in order:
                label = f'{trial:02d}-{identifier}-{backend}'
                record_path = output / 'records' / f'{label}.json'
                if record_path.exists():
                    continue
                # Separate pinned resource copies per trial/backend; source
                # homes, temporary directories and persistent caches are fresh
                # for every measured invocation. Preparation is not measured.
                assets = output / 'work' / f'{trial:02d}-{backend}' / 'resources'
                if not assets.exists():
                    shutil.copytree(bundle / 'resources', assets)
                work = output / 'work' / label
                for part in ('home/AppData/Roaming', 'home/AppData/Local', 'temp', 'cache'):
                    (work / part).mkdir(parents=True, exist_ok=True)
                stdout, stderr = output / 'raw' / f'{label}.json', output / 'raw' / f'{label}.stderr.log'
                stdout.parent.mkdir(exist_ok=True)
                command = [str(executable), '--assets', str(assets), '--python',
                           str(python if backend == 'python' else work / 'python-must-not-run.exe'),
                           '--source-backend', backend, '--json', 'search', args.query,
                           '--source', identifier, '--category', 'all', '--limit', str(args.limit),
                           '--timeout', str(args.timeout), '--parallel', '1']
                state.update(current=dict(source=identifier, backend=backend, trial=trial,
                                          started_utc=utc()), updated_utc=utc())
                save(output / 'state.json', state)
                started = time.monotonic()
                try:
                    with environment(dict(USERPROFILE=work / 'home', APPDATA=work / 'home/AppData/Roaming',
                         LOCALAPPDATA=work / 'home/AppData/Local', TEMP=work / 'temp', TMP=work / 'temp',
                         TORRENT_SEARCH_CACHE_DIR=work / 'cache')):
                        measurement, report = meter.measure(command, timeout=args.timeout + 20,
                            parse_failed_report=True, stdout_path=stdout, stderr_path=stderr)
                    details = report_details(report, identifier)
                    if report is not None and (report['query'] != args.query or report['category'] != 'all'):
                        raise ValueError('Measured query or category changed')
                    if backend == 'rust' and any(name.lower().startswith('python')
                            for name in measurement['observed_process_names']):
                        raise ValueError('Native benchmark unexpectedly spawned Python')
                    measurement.pop('stdout', None)
                    measurement.pop('error', None)
                    record = dict(**measurement, **details)
                except Exception as error:
                    record = dict(status='measurement_failed', identities=[], unique_results=0,
                                  configuration_diagnostic=False, error=f'{type(error).__name__}: {error}',
                                  harness_wall_ms=(time.monotonic() - started) * 1000)
                record.update(source=identifier, backend=backend, trial=trial, completed_utc=utc(),
                              raw_report=str(stdout.relative_to(output)), diagnostics=str(stderr.relative_to(output)))
                save(record_path, record)
                completed += 1
                state.update(completed_measurements=completed, updated_utc=utc())
                save(output / 'state.json', state)
                print(json.dumps(dict(done=completed, total=expected, source=identifier, backend=backend,
                                      trial=trial, status=record['status'], results=record.get('unique_results'),
                                      wall_ms=round(record.get('wall_ms', record.get('harness_wall_ms', 0)), 1))), flush=True)
            summarize(output, settings, sources)
    rows, count = summarize(output, settings, sources)
    if count != expected:
        raise ValueError(f'Incomplete benchmark: {count}/{expected}')
    failures = [read(path) for path in (output / 'records').glob('*.json') if read(path)['status'] == 'measurement_failed']
    state.update(phase='completed' if not failures else 'completed_with_measurement_failures',
                 current=None, completed_utc=utc(), measurement_failures=len(failures),
                 sources_with_comparable_trials=sum(r['comparable_trials'] > 0 for r in rows))
    save(output / 'state.json', state)
    print(json.dumps(state), flush=True)
    return 0 if not failures else 1


if __name__ == '__main__':
    raise SystemExit(main())
