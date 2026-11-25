#!/usr/bin/env python3
"""Turns the JSON lines that bench.py --json wrote into a Markdown table.

Example:
    report.py results.jsonl > results.md
"""

import json
import sys


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: report.py results.jsonl")
    with open(sys.argv[1]) as f:
        results = [json.loads(line) for line in f if line.strip()]
    print("| Browser | Tabs | First page | All pages | Memory (PSS) |")
    print("|---|---:|---:|---:|---:|")
    for r in sorted(results, key=lambda r: (r["tabs"], r["browser"])):
        m = r["median"]
        print(f"| {r['browser']} | {r['tabs']} | {m['first_page_s']:.2f} s | {m['all_pages_s']:.2f} s | {m['memory_mb']:.0f} MB |")
    runs = sorted({r["runs"] for r in results})
    print()
    print(f"Medians of {', '.join(map(str, runs))} runs each, after one warm-up run. Each run starts the browser with a "
          "fresh profile and opens every tab from the command line; memory is measured five seconds after the last "
          "page loaded.")


if __name__ == "__main__":
    main()
