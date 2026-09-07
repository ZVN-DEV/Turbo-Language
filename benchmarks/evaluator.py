#!/usr/bin/env python3
"""Paired native benchmark evidence; never confuses a subset with qualification.

Python 3.10+, standard library only. Process measurement supports macOS/Linux.
See EVALUATOR.md for metric scope, limitations, protocol and exit codes.
"""

import argparse
from collections import Counter, defaultdict
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path(__file__).with_name("evaluator-cases.json")
IMPLEMENTED = {"fib", "wordcount", "buffer_scan", "hashmap_churn", "particle_update", "string_tokens", "tree_walk", "json_transform"}


class EvaluationError(Exception):
    pass


@contextmanager
def measurement_lock(path=ROOT / "benchmarks/.evaluator.lock"):
    """One evaluator per checkout; kernel releases the lock on process death.

    Keep the file: unlinking a lock file can split contenders across inodes.
    This does not assert that other programs on the host are idle.
    """
    if sys.platform not in ("darwin", "linux"):
        raise EvaluationError("native evaluator currently supports macOS/Linux")
    import fcntl
    descriptor = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "rb") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise EvaluationError("another evaluator is active in this checkout") from error
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    hasher = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def rss_bytes(raw, system):
    if system == "darwin":
        return int(raw)
    if system == "linux":
        return int(raw) * 1024
    raise ValueError("per-child RSS is currently supported only on macOS/Linux")


def stop_owned_child(child):
    """Fall back to the known child PID if its process group rejects signaling.

    The caller has not reaped this child, so the PID cannot be reused. A group
    denial never turns a timeout/output-limit sample into success.
    """
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except PermissionError:
        pid, status, usage = os.wait4(child.pid, os.WNOHANG)
        if pid:
            child.returncode = os.waitstatus_to_exitcode(status)
            return status, usage
        try:
            # Do not use Popen.kill(): its internal poll can consume the
            # resource-usage record before this collector receives wait4.
            os.kill(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    _, status, usage = os.wait4(child.pid, 0)
    child.returncode = os.waitstatus_to_exitcode(status)
    return status, usage


def run_process(argv, *, cwd=None, env=None, timeout_s=30, max_output_bytes=1_048_576):
    """Measure one owned child with wait4, not cumulative RUSAGE_CHILDREN.

    Wall time includes process launch/exec and up to 1ms polling delay. RSS is
    the OS high-water mark, NOT live heap or allocation count. Temporary files
    avoid pipe deadlocks; output is capped and a noisy child is terminated.
    """
    if sys.platform not in ("darwin", "linux") or not hasattr(os, "wait4"):
        raise ValueError("native evaluator requires macOS/Linux wait4")
    if (not math.isfinite(timeout_s) or timeout_s <= 0
            or isinstance(max_output_bytes, bool) or not isinstance(max_output_bytes, int)
            or max_output_bytes <= 0):
        raise ValueError("timeout and output limit must be finite and positive")
    sample = dict(command=[str(a) for a in argv], pid=None, status="launch_error",
                  exit_code=None, elapsed_ns=None, peak_rss_bytes=None,
                  stdout="", stderr="", stdout_sha256=None, stderr_sha256=None)
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        start = time.perf_counter_ns()
        try:
            child = subprocess.Popen(sample["command"], cwd=cwd, env=env,
                                     stdin=subprocess.DEVNULL, stdout=stdout,
                                     stderr=stderr, start_new_session=True)
        except OSError as error:
            sample.update(stderr=str(error), elapsed_ns=time.perf_counter_ns() - start)
            return sample
        sample["pid"] = child.pid
        outcome = None
        try:
            while True:
                pid, status, usage = os.wait4(child.pid, os.WNOHANG)
                if pid:
                    child.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.perf_counter_ns() - start >= timeout_s * 1e9:
                    outcome = "timeout"
                elif max(os.fstat(stdout.fileno()).st_size,
                         os.fstat(stderr.fileno()).st_size) > max_output_bytes:
                    outcome = "output_limit"
                if outcome:
                    # Child has not been reaped: its PID/session still belong
                    # to this invocation. Kill only that owned process group.
                    status, usage = stop_owned_child(child)
                    break
                time.sleep(.001)
        finally:
            # Also reap on cancellation/errors; poll handles an already-reaped
            # child without targeting an unrelated process group.
            if child.returncode is None and child.poll() is None:
                stop_owned_child(child)
        sample.update(elapsed_ns=time.perf_counter_ns() - start,
                      exit_code=child.returncode,
                      peak_rss_bytes=rss_bytes(usage.ru_maxrss, sys.platform))
        for name, stream in (("stdout", stdout), ("stderr", stderr)):
            stream.seek(0)
            raw = stream.read(max_output_bytes + 1)
            if len(raw) > max_output_bytes:
                outcome = outcome or "output_limit"
            else:
                sample[name + "_sha256"] = digest(raw)
            sample[name] = raw[:max_output_bytes].decode("utf-8", errors="replace")
        sample["status"] = outcome or ("ok" if child.returncode == 0 else "failed")
        return sample


def check_output(sample, expected):
    if sample["status"] != "ok":
        raise EvaluationError(f"child {sample['status']}: {sample['stderr']}")
    if sample["stdout_sha256"] != digest(expected):
        raise EvaluationError(f"output mismatch: {sample['stdout']!r}; expected {expected!r}")
    if sample["stderr"]:
        raise EvaluationError(f"unexpected stderr: {sample['stderr']!r}")


def parse_allocation_profile(sample, expected, *, require_zero_live=False):
    """Only the explicit instrumented phase may accept an observer stderr line."""
    check_output(dict(sample, stderr=""), expected)
    lines = sample["stderr"].splitlines()
    prefix = "TURBO_ALLOC_PROFILE "
    if len(lines) != 1 or not lines[0].startswith(prefix):
        raise EvaluationError("missing, duplicate or noisy allocation profile")
    try:
        profile = json.loads(lines[0][len(prefix):])
    except ValueError as error:
        raise EvaluationError("invalid allocation profile JSON") from error
    if (not isinstance(profile, dict) or type(profile.get("schema_version")) is not int
            or profile.get("schema_version") != 1
            or profile.get("coverage") != "shared_header_arc" or profile.get("valid") is not True
            or profile.get("scope_end") != "entry_return"):
        raise EvaluationError("invalid allocation profile scope or observer state")
    counters = ("allocations heap_allocations arena_allocations heap_frees arena_reclaims "
                "total_data_bytes total_header_bytes live_allocations peak_live_allocations "
                "live_data_bytes peak_live_data_bytes live_header_bytes retain_calls release_calls "
                "retain_ops release_ops unknown_frees errors peak_observer_bytes").split()
    if any(type(profile.get(key)) is not int or not 0 <= profile[key] <= 2**64 - 1 for key in counters):
        raise EvaluationError("missing or invalid allocation counters")
    p = profile
    if (p["errors"] or p["unknown_frees"]
            or p["allocations"] != p["heap_allocations"] + p["arena_allocations"]
            or p["allocations"] != p["heap_frees"] + p["arena_reclaims"] + p["live_allocations"]
            or p["heap_frees"] > p["heap_allocations"] or p["arena_reclaims"] > p["arena_allocations"]
            or not p["live_allocations"] <= p["peak_live_allocations"] <= p["allocations"]
            or not p["live_data_bytes"] <= p["peak_live_data_bytes"] <= p["total_data_bytes"]
            or p["live_header_bytes"] > p["total_header_bytes"]):
        raise EvaluationError("allocation accounting invariants failed")
    if require_zero_live and any(p[key] for key in ("live_allocations", "live_data_bytes", "live_header_bytes")):
        raise EvaluationError("workload left live allocations at entry return")
    return profile


def percentile(values, quantile):
    if not values or not 0 <= quantile <= 1:
        raise ValueError("percentile requires values and a quantile in [0,1]")
    ordered = sorted(values)
    index = (len(ordered) - 1) * quantile
    lo, hi = math.floor(index), math.ceil(index)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (index - lo)


def _ratio(rows):
    return statistics.median(p["turbo_ns"] / p["rust_ns"] for p in rows)


def _geomean(values):
    return math.exp(statistics.mean(math.log(v) for v in values))


def summarize(cases, *, draws=2000, seed=20260906):
    """Hierarchical paired bootstrap, with common batch resampling across cases.

    Point estimator is the median of within-pair elapsed ratios, not a ratio
    chosen from independently fastest runs. Workload weights are equal.
    """
    if not cases or draws < 1:
        raise ValueError("nonempty cases and positive bootstrap count required")
    grouped = {}
    summaries = {}
    batch_ids = None
    for name, rows in sorted(cases.items()):
        if not rows:
            raise ValueError(f"empty case {name}")
        batches = defaultdict(list)
        seen = set()
        for row in rows:
            for key in ("turbo_ns", "rust_ns"):
                v = row[key]
                if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v <= 0:
                    raise ValueError("durations must be finite positive numbers")
            identity = (row["batch"], row["pair"])
            if identity in seen:
                raise ValueError("duplicate batch/pair identity")
            seen.add(identity)
            batches[row["batch"]].append(row)
        ids = sorted(batches)
        if batch_ids is not None and ids != batch_ids:
            raise ValueError("cases must contain the same completed batches")
        batch_ids = ids
        grouped[name] = batches
        summaries[name] = dict(ratio=_ratio(rows), pair_count=len(rows), batch_count=len(ids))
        for language in ("turbo", "rust"):
            values = [p[language + "_ns"] for p in rows]
            summaries[name][language + "_median_ns"] = statistics.median(values)
            summaries[name][language + "_p95_ns"] = percentile(values, .95)
    rng = random.Random(seed)
    bootstraps = {name: [] for name in grouped}
    aggregate = []
    for _ in range(draws):
        selected = rng.choices(batch_ids, k=len(batch_ids))
        ratios = []
        for name, batches in grouped.items():
            rows = [row for batch in selected
                    for row in rng.choices(batches[batch], k=len(batches[batch]))]
            ratio = _ratio(rows)
            bootstraps[name].append(ratio)
            ratios.append(ratio)
        aggregate.append(_geomean(ratios))
    for name, values in bootstraps.items():
        summaries[name]["ratio_ci95"] = [percentile(values, .025), percentile(values, .975)]
    return dict(cases=summaries,
                geomean_ratio=_geomean(s["ratio"] for s in summaries.values()),
                geomean_ci95=[percentile(aggregate, .025), percentile(aggregate, .975)],
                estimator="median paired ratios; equal workload weights",
                bootstrap=dict(draws=draws, seed=seed, method="batch then paired samples"))


def cpu_verdict(summary, *, blockers=(), geomean_limit=1.15, individual_limit=1.35):
    intervals = {name: (value["ratio_ci95"], individual_limit)
                 for name, value in summary["cases"].items()}
    intervals["geomean"] = (summary["geomean_ci95"], geomean_limit)
    failed = [name for name, (ci, limit) in intervals.items() if ci[0] > limit]
    uncertain = [name for name, (ci, limit) in intervals.items() if ci[0] <= limit < ci[1]]
    observed = "fail" if failed else "inconclusive" if uncertain else "pass"
    return dict(status="incomplete" if blockers else observed,
                observed_subset_status=observed, failures=failed,
                inconclusive=uncertain, blockers=list(blockers))


def source_path(relative):
    path = (ROOT / relative).resolve()
    if not path.is_relative_to(ROOT) or not path.is_file():
        raise ValueError(f"source outside repository or missing: {relative}")
    return path


def load_manifest(path=MANIFEST):
    manifest = json.loads(Path(path).read_text())
    if (not isinstance(manifest, dict) or manifest.get("schema_version") != 1
            or not isinstance(manifest.get("cases"), dict) or not manifest["cases"]
            or not isinstance(manifest.get("suite_version"), str)):
        raise ValueError("unsupported or empty evaluator manifest")
    for name, case in manifest["cases"].items():
        if (not isinstance(case, dict) or case.get("status") not in ("runnable", "pending_fixture")
                or case.get("controlled_status") not in ("pending_capability", "runnable")
                or case.get("category") not in ("cpu", "application", "service")):
            raise ValueError(f"invalid case status: {name}")
        if case["status"] == "runnable" and name not in IMPLEMENTED:
            raise ValueError(f"no runner implementation: {name}")
        if type(case.get("require_zero_live", False)) is not bool:
            raise ValueError(f"invalid allocation contract: {name}")
        reasons = case.get("qualification_blockers", [])
        if not isinstance(reasons, list) or any(not isinstance(reason, str) or not reason.strip() for reason in reasons):
            raise ValueError(f"invalid qualification blockers: {name}")
        method = case.get("rust_build", "rustc")
        if method not in ("rustc", "workspace_example"):
            raise ValueError(f"unsupported Rust build method: {name}")
        if method == "workspace_example" and case.get("rust_source") != "turbo/crates/turbo-cli/examples/bench_json_transform.rs":
            raise ValueError("workspace example source must identify the compiled JSON reference")
        parameters = case.get("environment", {})
        if (not isinstance(parameters, dict)
                or not set(parameters) <= {"TURBO_BENCH_SIZE", "TURBO_BENCH_STEPS"}
                or any(not isinstance(v, str) or not v.isascii() or not v.isdecimal()
                       or not 0 < int(v) <= 2**63 - 1 for v in parameters.values())):
            raise ValueError(f"invalid benchmark parameters: {name}")
        if name == "particle_update" and case["status"] == "runnable":
            if (set(parameters) != {"TURBO_BENCH_SIZE", "TURBO_BENCH_STEPS"}
                    or int(parameters["TURBO_BENCH_SIZE"]) > 10000
                    or int(parameters["TURBO_BENCH_STEPS"]) > 65536):
                raise ValueError("particle parameters outside exact lattice contract")
        if name == "tree_walk" and case["status"] == "runnable":
            if (set(parameters) != {"TURBO_BENCH_SIZE", "TURBO_BENCH_STEPS"}
                    or int(parameters["TURBO_BENCH_SIZE"]) > 20
                    or int(parameters["TURBO_BENCH_STEPS"]) > 64):
                raise ValueError("tree parameters outside workload contract")
        if name == "json_transform" and case["status"] == "runnable":
            if (set(parameters) != {"TURBO_BENCH_SIZE", "TURBO_BENCH_STEPS"}
                    or int(parameters["TURBO_BENCH_SIZE"]) > 4096
                    or int(parameters["TURBO_BENCH_STEPS"]) > 512):
                raise ValueError("JSON parameters outside workload contract")
            for required in ("turbo/benchmarks/gen_json_input.py", "turbo/Cargo.toml",
                             "turbo/Cargo.lock", "turbo/crates/turbo-cli/Cargo.toml"):
                if required not in case.get("source_sha256", {}):
                    raise ValueError(f"unfingerprinted JSON input/dependency contract: {required}")
        if name == "string_tokens" and case["status"] == "runnable":
            if (set(parameters) != {"TURBO_BENCH_STEPS"}
                    or int(parameters["TURBO_BENCH_STEPS"]) > 16777216):
                raise ValueError("string token parameters outside workload contract")
            if "turbo/benchmarks/string_tokens_corpus.txt" not in case.get("source_sha256", {}):
                raise ValueError("unfingerprinted string token corpus")
        if case["status"] == "runnable":
            for language in ("turbo", "rust"):
                relative = case.get(language + "_source")
                if not isinstance(relative, str) or relative not in case.get("source_sha256", {}):
                    raise ValueError(f"unfingerprinted source: {name}/{language}")
        for relative, expected_hash in case.get("source_sha256", {}).items():
            if file_digest(source_path(relative)) != expected_hash:
                raise ValueError(f"fixture changed; revise evaluator manifest explicitly: {relative}")
    return manifest


def wordcount_oracle(path):
    counts = Counter(Path(path).read_text(encoding="ascii").split())
    top = sorted(counts.items(), key=lambda pair: (-pair[1], pair[0]))[:20]
    return ("".join(f"{word} {count}\n" for word, count in top)
            + f"TOTAL {sum(counts.values())} {len(counts)}\n").encode()


def buffer_oracle(size):
    """Compose periodic affine checksum blocks; cross-tested against literal bytes.

    Avoid making the Python oracle perform 134M byte updates for the default
    fixture. This algebra is independent of the native loop implementations.
    """
    if size <= 0:
        raise ValueError("buffer size must be positive")
    modulus = 1_000_000_007
    checksum = 0
    offset = 0
    for step in range(4):
        offset += step + 1
        block = [(i * 17 + 23 + offset) % 256 for i in range(256)]
        a, b = 1, 0
        for value in block:
            a, b = a * 33 % modulus, (b * 33 + value) % modulus
        count = size // 256
        mul, addend = 1, 0
        while count:
            if count & 1:
                mul, addend = a * mul % modulus, (a * addend + b) % modulus
            a, b = a * a % modulus, (a * b + b) % modulus
            count >>= 1
        checksum = (mul * checksum + addend) % modulus
        for value in block[:size % 256]:
            checksum = (checksum * 33 + value) % modulus
    return f"{checksum}\n{(23 + offset) % 256}\n{((size - 1) * 17 + 23 + offset) % 256}\n".encode()


def hashmap_oracle(steps):
    if steps <= 0:
        raise ValueError("churn steps must be positive")
    counts = {}
    state = 7
    for step in range(steps):
        state = (state * 1103515245 + 12345) % 2147483648
        key = state % 4096
        counts[key] = counts.get(key, 0) + 1
        if step % 7 == 0:
            counts.pop((key + 17) % 4096, None)
    checksum = sum((key + 1) * value for key, value in counts.items())
    return f"{checksum}\n{checksum}\n{len(counts)}\n{len(counts)}\n".encode()


def json_transform_oracle(path, size, rounds):
    """Validate the frozen schema, transform once, then weight the multiset.

    The native programs process every round. The independent oracle weights
    each projected record; U+2028 is string content, never a record delimiter.
    """
    if not 0 < size <= 4096 or not 0 < rounds <= 512:
        raise ValueError("JSON parameters outside workload contract")
    text = Path(path).read_bytes().decode("utf-8")
    if not text.endswith("\n"):
        raise ValueError("JSON input must end in a record newline")
    lines = text[:-1].split("\n")
    if len(lines) != size:
        raise ValueError("JSON input record count mismatch")
    counts, total_bytes = Counter(), 0
    for line in lines:
        row = json.loads(line)
        if (not isinstance(row, dict) or type(row.get("id")) is not int
                or not -4096 <= row["id"] <= 4096 or type(row.get("score")) is not int
                or not -1000 <= row["score"] <= 1000 or type(row.get("active")) is not bool
                or not isinstance(row.get("title"), str) or "\0" in row["title"]):
            raise ValueError("JSON input outside frozen record schema")
        row["title"].encode("utf-8")
        if row["active"] and row["id"] % 5 != 0:
            projected = dict(id=row["id"], score=row["score"] * 3 + row["id"], title="task:" + row["title"])
            encoded = json.dumps(projected, ensure_ascii=False, separators=(",", ":"))
            counts[encoded] += rounds
            total_bytes += len(encoded.encode("utf-8")) * rounds
    result = "".join(f"{record}\t{counts[record]}\n" for record in sorted(counts))
    return (result + f"TOTAL {sum(counts.values())} {total_bytes} {size * rounds}\n").encode("utf-8")


def tree_oracle(depth, rounds):
    """Independent flat, breadth-first construction and bottom-up reduction.

    Native workloads allocate recursive enum nodes and walk/drop recursively.
    The oracle instead assigns children by heap indices in a flat array.
    """
    if not 0 < depth <= 20 or not 0 < rounds <= 64:
        raise ValueError("tree parameters outside workload contract")
    nodes = (1 << (depth + 1)) - 1
    parents = nodes // 2
    checksum = 0
    for step in range(rounds):
        values = [0] * nodes
        values[0] = 7 + step * 7919
        for index in range(parents):
            seed = values[index]
            values[index * 2 + 1] = (seed * 48271 + 17) % 2147483647
            values[index * 2 + 2] = (seed * 69621 + 31) % 2147483647
        for index in range(nodes - 1, -1, -1):
            value = values[index] % 1000
            if index < parents:
                value = (values[index * 2 + 1] * 33 + value * 17
                         + values[index * 2 + 2] * 97) % 1_000_000_007
            values[index] = value
        checksum = (checksum * 65599 + values[0]) % 1_000_000_007
    return f"{checksum}\n{nodes * rounds}\n{rounds}\n".encode()


def string_tokens_oracle(path, steps):
    """Count each frozen record once and weight by its cyclic visit count.

    Preserve Unicode except the explicit em-dash replacement; reject whitespace
    outside the shared native trim subset rather than conceal JIT/AOT drift.
    """
    text = Path(path).read_bytes().decode("utf-8")
    if (not 0 < steps <= 16777216 or not text or not text.endswith("\n") or "\0" in text
            or any(c.isspace() and c not in " \t\r\n" for c in text)):
        raise ValueError("string corpus outside UTF-8/ASCII-margin workload contract")
    lines = text[:-1].split("\n")
    counts, input_bytes = Counter(), 0
    rounds, remainder = divmod(steps, len(lines))
    for i, line in enumerate(lines):
        visits = rounds + (i < remainder)
        input_bytes += len(line.encode()) * visits
        if not visits:
            continue
        for field in line.split("|"):
            token = field.strip(" \t\r\n").replace("INFO:", "info:").replace("WARN:", "warn:")
            token = token.replace("—", "-")
            if token:
                counts[token] += visits
    result = "".join(f"{key} {counts[key]}\n" for key in sorted(counts))
    return (result + f"TOTAL {sum(counts.values())} {len(counts)} {input_bytes}\n").encode()


def particle_oracle(size, steps):
    """Closed-form integer trajectory, independent of native f64 update loops.

    Position is represented in 1/65536 units. Initial position, velocity and
    acceleration are seeded integers divided by 1024; dt is 1/64. At <=65536
    steps the largest lattice coordinate is below 2**42, hence exactly
    representable in f64, including intermediates. No tolerance hides drift.
    """
    if not 0 < size <= 10000 or not 0 < steps <= 65536:
        raise ValueError("particle parameters outside exact lattice contract")
    seed, checksum = 7, 0
    for _ in range(size):
        values = []
        for _ in range(6):
            seed = (seed * 48271) % 2147483647
            values.append(seed % 2048 - 1024)
        x, y, vx, vy, ax, ay = values
        triangle = steps * (steps + 1) // 2
        final = (64 * x + steps * vx + triangle * ax,
                 64 * y + steps * vy + triangle * ay,
                 64 * (vx + steps * ax), 64 * (vy + steps * ay), 64 * ax, 64 * ay)
        for value in final:
            checksum = (checksum * 33 + value) % 1_000_000_007
    return f"{checksum}\n{size}\n{steps}\n".encode()


def tool_output(command):
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=10, check=True)
        return result.stdout.decode(errors="replace").strip()
    except (OSError, subprocess.SubprocessError) as error:
        return f"unavailable: {error}"


def working_tree_status():
    """Porcelain records include staged, unstaged and untracked paths.

    Preserve Git's quoted path representation (including unusual filenames)
    rather than splitting paths on spaces or dumping environment contents.
    """
    return tool_output(["git", "status", "--porcelain=v1", "--untracked-files=all"]).splitlines()


def cargo_example_artifact(stdout, source):
    """Select the executable Cargo actually built, including configured targets."""
    matches = []
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if not isinstance(message, dict) or message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target", {})
        if (isinstance(target, dict) and isinstance(target.get("kind"), list)
                and "example" in target["kind"]
                and isinstance(target.get("src_path"), str)
                and Path(target["src_path"]).resolve() == source.resolve()
                and isinstance(message.get("executable"), str)):
            matches.append(Path(message["executable"]).resolve())
    if len(matches) != 1 or not matches[0].is_file():
        raise EvaluationError("Cargo must report exactly one existing executable for the JSON reference")
    return matches[0]


def rust_host_target(version):
    hosts = [line.removeprefix("host: ") for line in version.splitlines() if line.startswith("host: ")]
    if len(hosts) != 1 or not hosts[0] or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-" for c in hosts[0]):
        raise EvaluationError("Rust compiler did not report one valid host target")
    return hosts[0]


def prepare_case(name, case, work, compiler, emit):
    commands = {}
    builds = {}
    env = os.environ.copy()
    env["LC_ALL"] = "C"
    parameters = case.get("environment", {})
    if not set(parameters) <= {"TURBO_BENCH_SIZE", "TURBO_BENCH_STEPS"}:
        raise EvaluationError("unsupported benchmark environment override")
    env.update(parameters)
    data = None
    if name == "wordcount":
        data = work / "wordcount_input.txt"
        generator = source_path("turbo/benchmarks/gen_wordcount_input.py")
        generated = run_process([sys.executable, generator, data, "5"], timeout_s=60)
        emit(dict(kind="input_generation", case=name, sample=generated))
        check_output(generated, b"")
        env["WORDCOUNT_INPUT"] = str(data)
        expected = wordcount_oracle(data)
    elif name == "json_transform":
        data = work / "json_transform_input.ndjson"
        generator = source_path("turbo/benchmarks/gen_json_input.py")
        generated = run_process([sys.executable, generator, data, env["TURBO_BENCH_SIZE"]], timeout_s=60)
        emit(dict(kind="input_generation", case=name, sample=generated))
        check_output(generated, b"")
        env["JSON_TRANSFORM_INPUT"] = str(data)
        expected = json_transform_oracle(data, int(env["TURBO_BENCH_SIZE"]), int(env["TURBO_BENCH_STEPS"]))
    elif name == "string_tokens":
        data = source_path("turbo/benchmarks/string_tokens_corpus.txt")
        env["STRING_TOKENS_INPUT"] = str(data)
        expected = string_tokens_oracle(data, int(env["TURBO_BENCH_STEPS"]))
    elif name == "buffer_scan":
        expected = buffer_oracle(int(env["TURBO_BENCH_SIZE"]))
    elif name == "hashmap_churn":
        expected = hashmap_oracle(int(env["TURBO_BENCH_STEPS"]))
    elif name == "particle_update":
        expected = particle_oracle(int(env["TURBO_BENCH_SIZE"]), int(env["TURBO_BENCH_STEPS"]))
    elif name == "tree_walk":
        expected = tree_oracle(int(env["TURBO_BENCH_SIZE"]), int(env["TURBO_BENCH_STEPS"]))
    else:
        expected = b"102334155\n"
    for language in ("turbo", "rust"):
        binary = work / (name + "-" + language)
        source = source_path(case[language + "_source"])
        method = case.get("rust_build", "rustc") if language == "rust" else "turbo"
        build_env = os.environ.copy()
        if method == "workspace_example":
            rustc = shutil.which("rustc") or "rustc"
            host_target = rust_host_target(tool_output([rustc, "--version", "--verbose"]))
            overrides = {"CARGO_PROFILE_RELEASE_LTO": "false", "CARGO_ENCODED_RUSTFLAGS": "",
                         "RUSTC": rustc, "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""}
            build_env.update(overrides)
            # Empty encoded flags take precedence over inherited/config rustflags.
            # Cargo's artifact message, not a guessed host path, selects the output.
            command = [shutil.which("cargo") or "cargo", "rustc", "--release", "--locked", "--offline",
                       "--message-format=json", "--target", host_target,
                       "--manifest-path", ROOT / "turbo/Cargo.toml", "--target-dir", ROOT / "turbo/target",
                       "-p", "turbo-cli", "--example", source.stem, "--", "-C", "opt-level=3",
                       "-C", "target-cpu=native", "-C", "overflow-checks=off"]
        else:
            command = ([compiler, "build", source, "-o", binary] if language == "turbo" else
                   [shutil.which("rustc") or "rustc", "-C", "opt-level=3", "-C",
                    "target-cpu=native", "-C", "overflow-checks=off", source, "-o", binary])
        built = run_process(command, cwd=ROOT, env=build_env, timeout_s=600 if method == "workspace_example" else 180)
        emit(dict(kind="build", case=name, language=language, sample=built))
        if built["status"] != "ok":
            raise EvaluationError(f"{name}/{language} build failed: {built['stderr']}")
        if method == "workspace_example":
            artifact = cargo_example_artifact(built["stdout"], source)
            shutil.copy2(artifact, binary)
        builds[language] = dict(elapsed_ns=built["elapsed_ns"], binary_bytes=binary.stat().st_size,
                                binary_sha256=file_digest(binary), command=built["command"], method=method)
        if method == "workspace_example":
            builds[language].update(build_environment=overrides,
                artifact_path=str(artifact),
                compile_scope="shared cached CLI workspace dependencies; not clean/minimal Rust compile time")
        commands[language] = [str(binary)]
    return dict(commands=commands, env=env, expected=expected, builds=builds,
                input_kind="file" if data else "generated_in_memory",
                logical_input_bytes=int(env["TURBO_BENCH_SIZE"]) if name == "buffer_scan" else None,
                input_sha256=file_digest(data) if data else None,
                input_bytes=data.stat().st_size if data else 0,
                expected_stdout=expected.decode(), expected_sha256=digest(expected))


def profile_case(name, spec, case, work, compiler, count, timeout, emit):
    binary = work / (name + "-profile")
    source = source_path(spec["turbo_source"])
    built = run_process([compiler, "build", source, "-o", binary], cwd=ROOT, timeout_s=180)
    emit(dict(kind="profile_build", case=name, sample=built))
    if built["status"] != "ok":
        raise EvaluationError(f"profile build failed: {built['stderr']}")
    result = dict(binary_sha256=file_digest(binary), build=built, samples=[])
    env = dict(case["env"], TURBO_ALLOC_PROFILE="1")
    for mode, command in (("aot", [str(binary)]), ("jit", [compiler, "run", str(source)])):
        for iteration in range(count):
            sample = run_process(command, cwd=work, env=env, timeout_s=timeout)
            emit(dict(kind="allocation_profile", case=name, mode=mode, iteration=iteration, sample=sample))
            profile = parse_allocation_profile(sample, case["expected"],
                                               require_zero_live=spec.get("require_zero_live", False))
            result["samples"].append(dict(mode=mode, iteration=iteration, counters=profile,
                instrumented_elapsed_ns=sample["elapsed_ns"], instrumented_peak_rss_bytes=sample["peak_rss_bytes"]))
    return result


def evaluate(args, manifest, emit):
    compiler = Path(args.compiler).resolve()
    if not compiler.is_file():
        raise EvaluationError("build turbo/target/release/turbolang before measuring")
    compiler_version = tool_output([str(compiler), "--version"])
    if not compiler_version.startswith("turbolang "):
        raise EvaluationError("could not verify compiler build flavor before measurement")
    if "+allocation-profile" in compiler_version:
        raise EvaluationError("instrumented compiler cannot supply timing baselines; use --profile-compiler separately")
    report = dict(schema_version=1, suite_version=manifest["suite_version"], status="running",
                  scope="initial measurement subset; G2.1 remains incomplete",
                  created_at=datetime.now(timezone.utc).isoformat(),
                  repository_revision=tool_output(["git", "rev-parse", "HEAD"]),
                  working_tree_status=working_tree_status(),
                  evaluator_sha256=file_digest(__file__), manifest_sha256=file_digest(MANIFEST),
                  host=dict(system=platform.system(), release=platform.release(),
                            architecture=platform.machine(), python=platform.python_version(),
                            cpu=tool_output(["sysctl", "-n", "machdep.cpu.brand_string"])
                            if sys.platform == "darwin" else platform.processor(),
                            power_and_load_control="not validated automatically"),
                  tools=dict(turbo=compiler_version,
                             timing_build=dict(flavor="standard", instrumented=False),
                             turbo_binary_sha256=file_digest(compiler),
                             compiler_source_revision="unverified; supplied binary is fingerprinted",
                             rust=tool_output(["rustc", "--version", "--verbose"]),
                             cc=tool_output([os.environ.get("CC", "cc"), "--version"])),
                  protocol=dict(samples=args.samples, batches=args.batches, warmups=args.warmups,
                                bootstrap_draws=args.bootstrap, seed=args.seed,
                                poll_interval_ns=1_000_000, wall_scope="process start through exit",
                                rss_scope="wait4 child high-water; not heap or sum of process tree"),
                  allocation_metrics=dict(status="not_instrumented", live_bytes=None,
                                          allocation_count=None, total_allocated_bytes=None),
                  cases={}, groups={})
    prepared = {}
    rows = {name: [] for name in args.cases}
    rng = random.Random(args.seed)
    try:
        with tempfile.TemporaryDirectory(prefix="turbo-evaluator-") as directory:
            work = Path(directory)
            for name in args.cases:
                spec = manifest["cases"][name]
                prepared[name] = prepare_case(name, spec, work, str(compiler), emit)
                report["cases"][name] = {key: value for key, value in prepared[name].items()
                                         if key not in ("env", "expected")}
                report["cases"][name].update(specification=spec, pairs=rows[name])
            for batch in range(args.batches):
                order = list(args.cases)
                rng.shuffle(order)
                for name in order:
                    case = prepared[name]
                    for phase, count in (("warmup", args.warmups), ("measurement", args.samples)):
                        for pair in range(count):
                            languages = ["turbo", "rust"]
                            rng.shuffle(languages)
                            measured = {}
                            for language in languages:
                                sample = run_process(case["commands"][language], cwd=work,
                                                     env=case["env"], timeout_s=args.timeout)
                                emit(dict(kind=phase, batch=batch, pair=pair, case=name,
                                          language=language, sample=sample))
                                # Validate every sample, including warmups; retain failure in JSONL first.
                                check_output(sample, case["expected"])
                                measured[language] = sample
                            if phase == "measurement":
                                rows[name].append(dict(batch=batch, pair=pair, order=languages,
                                    turbo_ns=measured["turbo"]["elapsed_ns"],
                                    rust_ns=measured["rust"]["elapsed_ns"],
                                    turbo_peak_rss_bytes=measured["turbo"]["peak_rss_bytes"],
                                    rust_peak_rss_bytes=measured["rust"]["peak_rss_bytes"]))
                    print(f"batch {batch + 1}/{args.batches}: {name} outputs verified", flush=True)
            if args.profile_compiler:
                profiler = str(Path(args.profile_compiler).resolve())
                version = tool_output([profiler, "--version"])
                if not version.startswith("turbolang ") or "+allocation-profile" not in version:
                    raise EvaluationError("--profile-compiler must be an allocation-profile build")
                report["allocation_metrics"].update(status="collecting", coverage="shared_header_arc",
                    build=dict(flavor="allocation-profile", instrumented=True),
                    compiler_version=version, compiler_sha256=file_digest(profiler), profiles={},
                    exclusions=["foreign/native library heaps", "Rust library temporaries", "non-header runtime storage",
                                "observer bookkeeping", "compiler allocations"])
                for name in args.cases:
                    report["allocation_metrics"]["profiles"][name] = profile_case(name, manifest["cases"][name],
                        prepared[name], work, profiler, args.profile_samples, args.timeout, emit)
                    report["allocation_metrics"]["status"] = "measured_partial"
        for category in sorted({manifest["cases"][name]["category"] for name in args.cases}):
            group = {name: rows[name] for name in args.cases
                     if manifest["cases"][name]["category"] == category}
            report["groups"][category] = summarize(group, draws=args.bootstrap, seed=args.seed)
        blockers = [f"{name}: {case['status']}" for name, case in manifest["cases"].items()
                    if name not in args.cases or case["status"] != "runnable"]
        blockers += ["controlled profiles pending G3", "allocation coverage incomplete" if args.profile_compiler else "allocation counters not instrumented",
                     "cross-host qualification not established", "host power/load conditions unvalidated"]
        blockers.append("compiler source provenance not independently attested")
        for name in args.cases:
            blockers.extend(f"{name}: {reason}" for reason in manifest["cases"][name].get("qualification_blockers", []))
        if args.samples < 20 or args.batches < 3 or args.warmups < 3 or args.bootstrap < 2000:
            blockers.append("smoke protocol below required sampling minimums")
        if any(min(p["turbo_ns"], p["rust_ns"]) < 200_000_000 for data in rows.values() for p in data):
            blockers.append("samples below 200ms; in-program batching required for qualification")
        report["qualification"] = (cpu_verdict(report["groups"]["cpu"], blockers=blockers)
                                     if "cpu" in report["groups"] else
                                     dict(status="incomplete", blockers=blockers + ["no CPU cases run"]))
        report["status"] = "measured"
    except (EvaluationError, OSError, ValueError) as error:
        report.update(status="failed", error=str(error), qualification=dict(status="not_evaluated"))
    except KeyboardInterrupt:
        report.update(status="interrupted", qualification=dict(status="not_evaluated"))
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", default="fib,wordcount,buffer_scan,hashmap_churn,particle_update,string_tokens,tree_walk,json_transform")
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--batches", type=int, default=3)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--bootstrap", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=20260906)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--compiler", default=str(ROOT / "turbo/target/release/turbolang"))
    parser.add_argument("--profile-compiler", help="separate allocation-profile build; never used for timing samples")
    parser.add_argument("--profile-samples", type=int, default=3)
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory; never overwritten")
    parser.add_argument("--check", action="store_true", help="require full plan qualification (currently incomplete)")
    args = parser.parse_args(argv)
    if min(args.samples, args.batches, args.warmups, args.bootstrap, args.profile_samples) < 1:
        parser.error("sample/batch/warmup/bootstrap counts must be positive")
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("timeout must be finite and positive")
    try:
        manifest = load_manifest()
        args.cases = args.cases.split(",")
        if (len(set(args.cases)) != len(args.cases) or not set(args.cases) <= IMPLEMENTED
                or any(manifest["cases"].get(name, {}).get("status") != "runnable" for name in args.cases)):
            parser.error("choose distinct implemented cases: " + ",".join(sorted(IMPLEMENTED)))
        args.output.mkdir(parents=True, exist_ok=False)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    with (args.output / "samples.jsonl").open("x") as stream:
        def emit(event):
            stream.write(json.dumps(event, ensure_ascii=True, allow_nan=False) + "\n")
            stream.flush()
        try:
            with measurement_lock():
                report = evaluate(args, manifest, emit)
        except (EvaluationError, OSError, ValueError) as error:
            report = dict(status="failed", error=str(error), qualification=dict(status="not_evaluated"))
        except KeyboardInterrupt:
            report = dict(status="interrupted", qualification=dict(status="not_evaluated"))
    with (args.output / "report.json").open("x") as stream:
        json.dump(report, stream, indent=2, ensure_ascii=True, allow_nan=False)
        stream.write("\n")
    print(f"{report['status']}; qualification={report['qualification']['status']}; {args.output}")
    if report["status"] == "interrupted":
        return 130
    if report["status"] != "measured":
        return 2
    if args.check:
        return {"pass": 0, "fail": 1, "incomplete": 3, "inconclusive": 4}[report["qualification"]["status"]]
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
