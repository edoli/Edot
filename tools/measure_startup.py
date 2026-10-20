"""Measure a Windows EXE's first visible window, or its time until exit.

Python standard library only. The timer includes process creation / OS loading.
Window mode observes WS_VISIBLE, not first paint or application-specific readiness.
Only the process launched for the current run is observed and closed.
"""

import argparse
import ctypes
from ctypes import wintypes
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time


class Windows:
    def __init__(self):
        self.api = ctypes.WinDLL("user32", use_last_error=True)
        self.callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        self.api.EnumWindows.argtypes = [self.callback, wintypes.LPARAM]
        self.api.EnumWindows.restype = wintypes.BOOL
        self.api.GetWindowThreadProcessId.argtypes = [
            wintypes.HWND, ctypes.POINTER(wintypes.DWORD)
        ]
        self.api.GetWindowThreadProcessId.restype = wintypes.DWORD
        self.api.IsWindowVisible.argtypes = [wintypes.HWND]
        self.api.IsWindowVisible.restype = wintypes.BOOL
        self.api.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
        self.api.GetWindowRect.restype = wintypes.BOOL
        self.api.PostMessageW.argtypes = [
            wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM
        ]
        self.api.PostMessageW.restype = wintypes.BOOL

    def windows(self, pid, visible_only=False):
        found = []

        @self.callback
        def visit(hwnd, _):
            owner = wintypes.DWORD()
            self.api.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value != pid:
                return True
            if visible_only:
                rect = wintypes.RECT()
                if (
                    not self.api.IsWindowVisible(hwnd)
                    or not self.api.GetWindowRect(hwnd, ctypes.byref(rect))
                    or rect.right <= rect.left
                    or rect.bottom <= rect.top
                ):
                    return True
            found.append(hwnd)
            return True

        if not self.api.EnumWindows(visit, 0):
            raise ctypes.WinError(ctypes.get_last_error())
        return found

    def close(self, pid):
        for hwnd in self.windows(pid, visible_only=True):
            # Recheck ownership: a window can disappear between enumeration and close.
            owner = wintypes.DWORD()
            self.api.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value == pid:
                self.api.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE


def positive_float(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be a finite number greater than zero")
    return number


def parse_args(argv):
    separator = argv.index("--") if "--" in argv else len(argv)
    parser = argparse.ArgumentParser(
        description=__doc__,
        usage="%(prog)s [options] EXE [-- EXE_ARGUMENTS ...]",
    )
    parser.add_argument("exe", type=Path, help="EXE path (quote paths with spaces)")
    parser.add_argument("--runs", type=int, default=5, help="number of runs (default: 5)")
    parser.add_argument("--mode", choices=("window", "exit"), default="window")
    parser.add_argument("--timeout", type=positive_float, default=15, help="seconds per run")
    parser.add_argument("--poll-ms", type=positive_float, default=5, help="window polling interval")
    parser.add_argument("--close-timeout", type=positive_float, default=5, help="seconds to close")
    parser.add_argument("--force-close", action="store_true", help="kill only the launched process if normal closing fails")
    parser.add_argument("--cwd", type=Path, help="working directory (default: EXE directory)")
    parser.add_argument("--env", action="append", default=[], metavar="KEY=VALUE", help="child environment override; repeatable")
    parser.add_argument("--json", type=Path, help="write a new JSON report; never overwrite a file")
    args = parser.parse_args(argv[:separator])
    args.program_args = argv[separator + 1 :]
    args.exe = args.exe.resolve()
    args.cwd = (args.cwd or args.exe.parent).resolve()
    if args.runs < 1:
        parser.error("--runs must be at least 1")
    if not args.exe.is_file() or args.exe.suffix.lower() != ".exe":
        parser.error(f"EXE not found: {args.exe}")
    if not args.cwd.is_dir():
        parser.error(f"working directory not found: {args.cwd}")
    args.environment = os.environ.copy()
    for setting in args.env:
        key, equal, value = setting.partition("=")
        if not key or not equal or "\0" in setting:
            parser.error("--env requires KEY=VALUE")
        for inherited in list(args.environment):
            if inherited.casefold() == key.casefold():
                del args.environment[inherited]
        args.environment[key] = value
    if args.json and args.json.exists():
        parser.error(f"report already exists: {args.json}")
    return args


def stop_process(process, windows, args):
    if process.poll() is not None:
        return False
    windows.close(process.pid)
    try:
        process.wait(timeout=args.close_timeout)
        return False
    except subprocess.TimeoutExpired:
        if not args.force_close:
            raise RuntimeError(
                f"PID {process.pid} did not close and was left running. "
                "Close it before retrying, or use --force-close with disposable data."
            )
        process.kill()  # Popen's original process handle; never kill by name/path.
        process.wait(timeout=args.close_timeout)
        return True


def measure(args, windows):
    started = time.perf_counter_ns()
    process = subprocess.Popen(
        [str(args.exe), *args.program_args],
        cwd=args.cwd,
        env=args.environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    elapsed = None
    forced = False
    try:
        deadline = started + int(args.timeout * 1_000_000_000)
        if args.mode == "exit":
            remaining = max(0, (deadline - time.perf_counter_ns()) / 1_000_000_000)
            try:
                code = process.wait(timeout=remaining)
            except subprocess.TimeoutExpired as error:
                raise RuntimeError(f"PID {process.pid}: exit timed out after {args.timeout}s") from error
            elapsed = (time.perf_counter_ns() - started) / 1_000_000
            if code != 0:
                raise RuntimeError(f"PID {process.pid}: exited with code {code}")
        else:
            while True:
                visible = windows.windows(process.pid, visible_only=True)
                observed = time.perf_counter_ns()
                if observed >= deadline:
                    raise RuntimeError(f"PID {process.pid}: no window within {args.timeout}s")
                code = process.poll()
                if code is not None:
                    raise RuntimeError(
                        f"PID {process.pid}: exited with code {code} before a window was observed. "
                        "Use --mode exit for CLI apps; existing-instance/child-process launchers are not measured."
                    )
                if visible:
                    elapsed = (observed - started) / 1_000_000
                    break
                time.sleep(min(args.poll_ms / 1000, (deadline - observed) / 1_000_000_000))
    finally:
        forced = stop_process(process, windows, args)
    if not forced and process.returncode != 0:
        raise RuntimeError(f"PID {process.pid}: exited with code {process.returncode} while closing")
    return {"pid": process.pid, "elapsed_ms": elapsed, "exit_code": process.returncode, "forced_close": forced}


def main():
    args = parse_args(sys.argv[1:])
    if sys.platform != "win32":
        raise SystemExit("This script requires Windows.")
    windows = Windows()
    report = {
        "exe": str(args.exe), "arguments": args.program_args, "cwd": str(args.cwd),
        "mode": args.mode, "runs": args.runs, "timeout_seconds": args.timeout,
        "poll_ms": args.poll_ms, "close_timeout_seconds": args.close_timeout,
        "force_close_requested": args.force_close, "samples": [],
    }
    failed = False
    output = None
    try:
        if args.json:
            args.json.parent.mkdir(parents=True, exist_ok=True)
            output = args.json.open("x", encoding="utf-8")
        print(f"EXE: {args.exe}")
        print("Metric: process launch -> " + ("first visible window" if args.mode == "window" else "process exit"))
        for run in range(args.runs):
            sample = measure(args, windows)
            report["samples"].append(sample)
            print(f"Run {run + 1}/{args.runs}: {sample['elapsed_ms']:.3f} ms" + (" (force-closed)" if sample["forced_close"] else ""), flush=True)
            time.sleep(0.5)
        times = [sample["elapsed_ms"] for sample in report["samples"]]
        summary = {
            "first_ms": times[0], "median_ms": statistics.median(times),
            "mean_ms": statistics.mean(times), "min_ms": min(times), "max_ms": max(times),
            "stdev_ms": statistics.stdev(times) if len(times) > 1 else 0,
        }
        report["summary"] = summary
        print("Summary (ms): " + ", ".join(f"{key.removesuffix('_ms')}={value:.3f}" for key, value in summary.items()))
    except (OSError, RuntimeError, subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        failed = True
        report["error"] = str(error) or "Interrupted"
        print(f"Error: {report['error']}", file=sys.stderr)
    finally:
        if output:
            with output:
                json.dump(report, output, ensure_ascii=False, indent=2)
                output.write("\n")
            print(f"Report: {args.json}")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
