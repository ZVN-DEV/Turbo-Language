"""Regression tests for the measurement/evidence contract (stdlib only)."""

import json
import contextlib
import io
import math
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import evaluator as ev


def pairs(ratio, batches=3, count=20):
    return [dict(batch=b, pair=i, turbo_ns=round(300_000_000 * ratio),
                 rust_ns=300_000_000) for b in range(batches) for i in range(count)]


class StatisticsTests(unittest.TestCase):
    def test_constant_ratio_and_geometric_mean(self):
        result = ev.summarize({"a": pairs(1.0), "b": pairs(1.21)}, draws=100)
        self.assertAlmostEqual(result["geomean_ratio"], 1.1)
        self.assertEqual(result["cases"]["b"]["ratio_ci95"], [1.21, 1.21])
        self.assertEqual(result["cases"]["a"]["pair_count"], 60)
        self.assertAlmostEqual(result["geomean_ci95"][1], 1.1)

    def test_seed_is_reproducible_and_batch_variation_survives(self):
        data = pairs(1.0)
        for p in data:
            if p["batch"] == 2:
                p["turbo_ns"] *= 2
        a = ev.summarize({"a": data}, draws=500, seed=5)
        self.assertEqual(a, ev.summarize({"a": data}, draws=500, seed=5))
        self.assertGreater(a["cases"]["a"]["ratio_ci95"][1], 1)

    def test_invalid_samples_are_not_silently_dropped(self):
        for duration in [0, -1, math.nan, math.inf, True]:
            with self.subTest(duration=duration), self.assertRaises(ValueError):
                ev.summarize({"a": [dict(batch=0, pair=0, turbo_ns=duration, rust_ns=1)]})
        with self.assertRaises(ValueError):
            ev.summarize({})
        with self.assertRaises(ValueError):
            ev.summarize({"a": []})
        with self.assertRaises(ValueError):
            ev.summarize({"a": pairs(1, batches=1), "b": pairs(1, batches=2)})

    def test_individual_outlier_cannot_hide_in_average(self):
        stats = ev.summarize({"fast": pairs(.6), "slow": pairs(1.5)}, draws=100)
        self.assertLess(stats["geomean_ratio"], 1.15)
        self.assertEqual(ev.cpu_verdict(stats)["status"], "fail")

    def test_uncertainty_is_not_a_pass(self):
        stats = ev.summarize({"a": pairs(1)}, draws=100)
        stats["geomean_ci95"] = [1.0, 1.2]
        self.assertEqual(ev.cpu_verdict(stats)["status"], "inconclusive")

    def test_pending_scope_prevents_qualification(self):
        stats = ev.summarize({"a": pairs(.9)}, draws=100)
        self.assertEqual(ev.cpu_verdict(stats)["status"], "pass")
        verdict = ev.cpu_verdict(stats, blockers=["missing fixtures", "unvalidated host"])
        self.assertEqual(verdict["status"], "incomplete")
        self.assertEqual(len(verdict["blockers"]), 2)

    def test_percentile_and_rss_units(self):
        self.assertEqual(ev.percentile([1, 2, 3], .5), 2)
        self.assertEqual(ev.rss_bytes(123, "darwin"), 123)
        self.assertEqual(ev.rss_bytes(123, "linux"), 123 * 1024)
        with self.assertRaises(ValueError):
            ev.rss_bytes(123, "win32")


@unittest.skipUnless(sys.platform in ("darwin", "linux"), "wait4 host contract")
class ProcessTests(unittest.TestCase):
    def test_measurement_lock_rejects_overlap_and_releases(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "lock"
            with ev.measurement_lock(path):
                script = ("import sys;sys.path.insert(0,sys.argv[1]);import evaluator as e;"
                          "\nwith e.measurement_lock(e.Path(sys.argv[2])): pass")
                sample = ev.run_process([sys.executable, "-c", script,
                                         str(Path(ev.__file__).parent), str(path)])
                self.assertEqual(sample["status"], "failed")
                self.assertIn("another evaluator", sample["stderr"])
            with ev.measurement_lock(path):
                pass

    def test_captures_child_not_runner_and_checks_output(self):
        sample = ev.run_process([sys.executable, "-c", "print('answer')"], timeout_s=5)
        self.assertEqual(sample["status"], "ok")
        self.assertGreater(sample["elapsed_ns"], 0)
        self.assertGreater(sample["peak_rss_bytes"], 0)
        self.assertEqual(sample["stdout"], "answer\n")
        ev.check_output(sample, b"answer\n")
        with self.assertRaises(ev.EvaluationError):
            ev.check_output(sample, b"not answer\n")

    def test_nonzero_exit_is_retained_and_rejected(self):
        sample = ev.run_process([sys.executable, "-c",
                                 "import sys; print('oops',file=sys.stderr);sys.exit(7)"])
        self.assertEqual(sample["exit_code"], 7)
        self.assertEqual(sample["status"], "failed")
        self.assertIn("oops", sample["stderr"])
        with self.assertRaises(ev.EvaluationError):
            ev.check_output(sample, b"")

    def test_matching_stdout_does_not_hide_stderr(self):
        sample = ev.run_process([sys.executable, "-c",
                                 "import sys;print('answer');print('warning',file=sys.stderr)"])
        with self.assertRaises(ev.EvaluationError):
            ev.check_output(sample, b"answer\n")

    def test_timeout_reaps_child(self):
        sample = ev.run_process([sys.executable, "-c", "import time;time.sleep(30)"], timeout_s=.05)
        self.assertEqual(sample["status"], "timeout")
        with self.assertRaises(ChildProcessError):
            os.waitpid(sample["pid"], os.WNOHANG)

    def test_bounded_output_and_launch_failure(self):
        sample = ev.run_process([sys.executable, "-c", "print('x'*4096)"], max_output_bytes=100)
        self.assertEqual(sample["status"], "output_limit")
        self.assertLessEqual(len(sample["stdout"]), 100)
        self.assertIsNone(sample["stdout_sha256"])
        sample = ev.run_process(["/nonexistent/turbo-evaluator-fixture"])
        self.assertEqual(sample["status"], "launch_error")
        self.assertIsNone(sample["peak_rss_bytes"])

    def test_invalid_limits_rejected(self):
        for timeout in [0, -1, math.nan, math.inf]:
            with self.assertRaises(ValueError):
                ev.run_process([sys.executable], timeout_s=timeout)


class OracleTests(unittest.TestCase):
    def test_wordcount_oracle_ties_and_empty(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input"
            path.write_bytes(b"b a b a\n c ")
            self.assertEqual(ev.wordcount_oracle(path), b"a 2\nb 2\nc 1\nTOTAL 5 3\n")
            path.write_bytes(b"")
            self.assertEqual(ev.wordcount_oracle(path), b"TOTAL 0 0\n")

    def test_manifest_preserves_missing_scope(self):
        manifest = ev.load_manifest()
        self.assertEqual(manifest["schema_version"], 1)
        self.assertIn("fib", manifest["cases"])
        self.assertIn("worker", manifest["cases"])
        self.assertEqual(manifest["cases"]["worker"]["status"], "pending_fixture")
        for case in manifest["cases"].values():
            self.assertEqual(case["controlled_status"], "pending_capability")

    def test_manifest_rejects_unregistered_and_unfingerprinted_cases(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "cases.json"
            manifest = ev.load_manifest()
            manifest["cases"]["invented"] = dict(manifest["cases"]["fib"])
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                ev.load_manifest(path)
            del manifest["cases"]["invented"]
            manifest["cases"]["fib"]["source_sha256"] = {}
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                ev.load_manifest(path)

    def test_fixture_hash_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            manifest = ev.load_manifest()
            hashes = manifest["cases"]["fib"]["source_sha256"]
            hashes[next(iter(hashes))] = "0" * 64
            path = Path(temp) / "cases.json"
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "fixture changed"):
                ev.load_manifest(path)


class EvidenceTests(unittest.TestCase):
    def invoke(self, directory, *, corrupt_at=None, check=False):
        prepared = dict(commands={"turbo": ["fixture-turbo"], "rust": ["fixture-rust"]},
                        env={}, expected=b"ok\n", builds={}, input_sha256=None,
                        input_bytes=0, expected_stdout="ok\n", expected_sha256=ev.digest(b"ok\n"))
        calls = []

        def sample(command, **kwargs):
            calls.append(command)
            output = b"wrong\n" if len(calls) == corrupt_at else b"ok\n"
            return dict(status="ok", exit_code=0, elapsed_ns=300_000_000,
                        peak_rss_bytes=1024, stdout=output.decode(), stderr="",
                        stdout_sha256=ev.digest(output))

        args = ["--cases", "fib", "--samples", "2", "--batches", "1", "--warmups", "1",
                "--bootstrap", "100", "--compiler", sys.executable, "--output", str(directory)]
        if check:
            args.append("--check")
        with patch.object(ev, "prepare_case", return_value=prepared), \
             patch.object(ev, "run_process", side_effect=sample), \
             patch.object(ev, "tool_output", return_value="test fixture"), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = ev.main(args)
        report = json.loads((directory / "report.json").read_text())
        events = [json.loads(line) for line in (directory / "samples.jsonl").read_text().splitlines()]
        return code, report, events

    def test_working_tree_provenance_includes_untracked_files(self):
        with tempfile.NamedTemporaryFile(dir=Path(__file__).parent,
                                         prefix="evaluator-provenance-", suffix=".txt") as stream:
            entries = ev.working_tree_status()
            self.assertTrue(any(Path(stream.name).name in row for row in entries))

    def test_smoke_can_measure_but_cannot_qualify(self):
        with tempfile.TemporaryDirectory() as temp:
            code, report, events = self.invoke(Path(temp) / "run", check=True)
            self.assertEqual(code, 3)
            self.assertEqual(report["status"], "measured")
            self.assertEqual(report["qualification"]["status"], "incomplete")
            self.assertIsNone(report["allocation_metrics"]["live_bytes"])
            self.assertEqual(len(events), 6)  # both warmups + both halves of each pair
            self.assertEqual(len(report["cases"]["fib"]["pairs"]), 2)
            self.assertEqual({event["language"] for event in events}, {"turbo", "rust"})

    def test_failure_is_preserved_before_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            code, report, events = self.invoke(Path(temp) / "run", corrupt_at=3)
            self.assertEqual(code, 2)
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["qualification"]["status"], "not_evaluated")
            self.assertEqual(events[-1]["sample"]["stdout"], "wrong\n")
            self.assertEqual(len(events), 3)

    def test_evidence_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "run"
            self.invoke(path)
            before = (path / "report.json").read_bytes()
            with self.assertRaises(SystemExit) as error:
                self.invoke(path)
            self.assertEqual(error.exception.code, 2)
            self.assertEqual(before, (path / "report.json").read_bytes())


if __name__ == "__main__":
    unittest.main()
