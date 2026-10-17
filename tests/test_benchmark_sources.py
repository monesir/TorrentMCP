"""Guard benchmark comparisons against false speed wins from errors or no work."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    'benchmark_sources', Path(__file__).resolve().parents[1] / 'scripts/benchmark-sources.py')
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class BenchmarkComparisons(unittest.TestCase):
    def test_only_successful_positive_matching_pairs_are_comparable(self):
        good = dict(status='completed', identities=['hash'], configuration_diagnostic=False)
        self.assertTrue(benchmark.comparable_pair(good, good))
        for change in [dict(status='failed'), dict(status='timed_out'),
                       dict(identities=[]), dict(identities=['different']),
                       dict(configuration_diagnostic=True)]:
            self.assertFalse(benchmark.comparable_pair(good, {**good, **change}))

    def test_jackett_configuration_row_is_preserved_but_not_counted(self):
        report = dict(sources=[dict(source_id='jackett', status='completed', result_count=1,
                                   elapsed_ms=1, error=None)],
                      results=[dict(title='Jackett: API key missing! Search: nirvana in fire',
                                    reference=dict(uri='http://127.0.0.1:9117'))])
        details = benchmark.report_details(report, 'jackett')
        self.assertTrue(details['configuration_diagnostic'])
        self.assertEqual(details['unique_results'], 0)
        self.assertEqual(details['result_count'], 1)

    def test_report_must_belong_to_exactly_the_selected_source(self):
        report = dict(sources=[dict(source_id='different')], results=[])
        with self.assertRaises(ValueError):
            benchmark.report_details(report, 'torrentdownloads')


if __name__ == '__main__':
    unittest.main()
