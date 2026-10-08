#!/usr/bin/env python3
"""Compare CAB byte/reader creation in fresh processes, including peak RSS.

Build with `cargo bench --bench streaming --no-run --locked` and pass the
resulting executable with --binary. GNU time measures each process separately.
"""
import argparse
import csv
import filecmp
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import tomllib
from datetime import datetime, timezone


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--csv", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--sizes", type=int, nargs="+", default=[65536, 1048576])
    parser.add_argument("--methods", nargs="+", choices=["stored", "mszip", "lzx", "quantum"],
                        default=["stored", "mszip", "lzx", "quantum"])
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--large-copy", type=int, default=33554432,
                        help="additional stored-only input size; 0 disables")
    parser.add_argument("--time-binary", default=shutil.which("time"))
    args = parser.parse_args()
    if not args.time_binary or args.repetitions < 1 or args.large_copy < 0 or any(size < 1 for size in args.sizes):
        parser.error("GNU time, positive sizes, and positive repetitions are required")
    binary = args.binary.resolve(strict=True)
    cases = [(method, corpus, size) for method in args.methods
             for corpus in ["repetitive", "random"] for size in args.sizes]
    if args.large_copy:
        cases += [("stored", corpus, args.large_copy) for corpus in ["repetitive", "random"]
                  if ("stored", corpus, args.large_copy) not in cases]
    rows = []
    with tempfile.TemporaryDirectory(prefix="cab-streaming-benchmark-") as directory:
        temp = Path(directory)
        for case_number, (method, corpus, size) in enumerate(cases):
            for repetition in range(args.repetitions):
                results = {}
                # Alternate execution order to reduce systematic thermal/cache bias.
                order = ["bytes", "reader"] if (case_number + repetition) % 2 == 0 else ["reader", "bytes"]
                for api in order:
                    output = temp / f"{api}.cab"
                    rss = temp / "rss.txt"
                    command = [args.time_binary, "-f", "%M", "-o", str(rss), str(binary),
                               "--api", api, "--method", method, "--corpus", corpus,
                               "--size", str(size), "--output", str(output)]
                    result = subprocess.run(command, check=True, capture_output=True, text=True)
                    row = json.loads(result.stdout)
                    row["repetition"] = repetition + 1
                    row["peak_rss_kib"] = int(rss.read_text().strip())
                    results[api] = row
                filecmp.clear_cache()
                if not filecmp.cmp(temp / "bytes.cab", temp / "reader.cab", shallow=False):
                    raise RuntimeError(f"CAB bytes differ: {method}/{corpus}/{size}")
                if results["bytes"]["sha256"] != results["reader"]["sha256"]:
                    raise RuntimeError("CAB digest disagreement")
                for api in ["bytes", "reader"]:
                    row = results[api]
                    row["byte_identical"] = True
                    rows.append(row)
                print(f"{method} {corpus} {size} run {repetition + 1}: identical", flush=True)
    args.csv.parent.mkdir(parents=True, exist_ok=True)
    fields = ["method", "corpus", "input_bytes", "api", "repetition", "cab_bytes",
              "seconds", "mib_per_second", "peak_rss_kib", "sha256", "byte_identical"]
    with args.csv.open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)
    def version(command):
        return subprocess.check_output(command, text=True).strip()
    def digest(path):
        with path.open("rb") as source:
            return hashlib.file_digest(source, "sha256").hexdigest()
    checkout = Path(__file__).resolve().parents[1]
    package_version = tomllib.loads((checkout / "Cargo.toml").read_text())["package"]["version"]
    cpu = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                if line.startswith("model name")), platform.processor())
    lines = ["# CAB streaming writer measurements", "",
             f"Measured UTC: {datetime.now(timezone.utc).isoformat(timespec='seconds')}", "",
             f"Host: {cpu}; {platform.platform()}; {os.cpu_count()} logical CPUs.",
             f"Compiler: `{version(['rustc', '--version'])}`.",
             f"Package: `ms-cabinet {package_version}`; release benchmark profile with thin LTO.",
             f"Binary SHA-256: `{digest(binary)}`.",
             f"Lockfile SHA-256: `{digest(checkout / 'Cargo.lock')}`.", "",
             "Each row summarizes independent fresh processes; throughput and peak RSS are medians.",
             "Runs use normal host scheduling without CPU affinity or frequency controls; small-case",
             "throughput differences can include first-use and scheduling noise. Timings do not",
             "include process startup. These measurements do not establish a universal speedup.",
             "All paired CAB files were compared byte-for-byte and have matching SHA-256 digests.",
             "Timing includes deterministic input generation, API registration, codec initialization,",
             "compression, and writes to a temporary file. It excludes file creation and final hashing;",
             "writes are not fsynced, so throughput includes the OS page cache rather than durable storage.",
             "GNU time peak RSS covers the whole child process, including startup, input generation,",
             "encoding, and fixed-buffer hashing. It is not an exact codec-workspace measurement.",
             "The byte API allocates and fills a complete input Vec inside the measured process.",
             "The reader API generates input in chunks up to 32 KiB; neither API retains output in RAM.",
             "LZX uses a 2 MiB window; Quantum uses level 6 and a 2 MiB window. Codec allocations,",
             "allocator reuse, and process baseline can dominate small-input peaks.",
             "Repetitive input repeats a fixed ASCII phrase; random input is deterministic xorshift32",
             "with seed 0x12345678. No source archives or historical benchmark evidence were modified.", "",
             "| Method | Corpus | Input MiB | CAB bytes | Byte MiB/s | Reader MiB/s | Byte peak MiB | Reader peak MiB |",
             "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for method, corpus, size in cases:
        selected = {api: [row for row in rows if (row['method'], row['corpus'], row['input_bytes'], row['api'])
                         == (method, corpus, size, api)] for api in ['bytes', 'reader']}
        speed = [statistics.median(row['mib_per_second'] for row in selected[api]) for api in ['bytes', 'reader']]
        peak = [statistics.median(row['peak_rss_kib'] for row in selected[api]) / 1024 for api in ['bytes', 'reader']]
        lines.append(f"| {method} | {corpus} | {size / 1048576:g} | {selected['bytes'][0]['cab_bytes']} | "
                     f"{speed[0]:.2f} | {speed[1]:.2f} | {peak[0]:.2f} | {peak[1]:.2f} |")
    lines += ["", f"Raw per-process samples: [{args.csv.name}]({args.csv.name}).", "",
              "Reproduce from the cabinet checkout using its development shell:", "", "```sh",
              "cargo bench --bench streaming --no-run --locked",
              "# Pass the executable path printed by Cargo (target directory may be shared).",
              "python3 scripts/benchmark-streaming.py --binary /path/to/streaming-executable \\",
              f"  --csv docs/{args.csv.name} --report docs/{args.report.name} \\",
              f"  --sizes {' '.join(map(str, args.sizes))} --repetitions {args.repetitions} --large-copy {args.large_copy} \\",
              f"  --methods {' '.join(args.methods)}", "```", ""]
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text("\n".join(lines))


if __name__ == "__main__":
    main()
