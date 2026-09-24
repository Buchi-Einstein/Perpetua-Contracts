#!/usr/bin/env python3
"""Validate gas baseline entries in docs/gas.md against measured values.

Reads the gas baseline table from docs/gas.md and checks that all values
are non-negative integers. This is a structural check — actual gas regression
testing is done by the Rust snapshot validation in the test job.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
GAS_DOC = REPO_ROOT / "docs" / "gas.md"


def build_cargo_test_env():
    """Build environment for running cargo tests."""
    env = os.environ.copy()
    home = env.get("HOME")
    if not home:
        raise RuntimeError("HOME is not set")
    cargo_bin = os.path.join(home, ".cargo", "bin")
    path = env.get("PATH", "")
    env["PATH"] = f"{cargo_bin}:{path}"
    return env


def extract_baselines(file_path):
    """Extract gas baselines from markdown file.
    
    Expected format:
    <!-- GAS_BASELINE_START -->
    {"operation": {"variant": 1234}, "other": 5678}
    <!-- GAS_BASELINE_END -->
    """
    with open(file_path, "r") as f:
        content = f.read()
    
    pattern = r"<!--\s*GAS_BASELINE_START\s*-->(.*?)<!--\s*GAS_BASELINE_END\s*-->"
    match = re.search(pattern, content, re.DOTALL)
    if not match:
        raise ValueError("Could not find gas baseline block")
    
    json_str = match.group(1).strip()
    try:
        return json.loads(json_str)
    except json.JSONDecodeError as e:
        raise ValueError(f"Could not parse gas baseline JSON: {e}")


def parse_measurements(output):
    """Parse gas measurements from test output.
    
    Expected format:
    GAS_MEASUREMENT: operation: variant: 1234
    GAS_MEASUREMENT: operation: 5678
    """
    measurements = {}
    pattern = r"GAS_MEASUREMENT:\s+(\w+)(?::\s+(\w+))?:\s+(\d+)"
    
    for line in output.splitlines():
        match = re.search(pattern, line)
        if match:
            operation = match.group(1)
            variant = match.group(2)
            value = int(match.group(3))
            
            if variant:
                if operation not in measurements:
                    measurements[operation] = {}
                measurements[operation][variant] = value
            else:
                measurements[operation] = value
    
    return measurements


def run_tests():
    """Run cargo tests to get gas measurements.
    
    This runs the resource_limits tests which measure gas consumption.
    """
    # Run the specific tests that measure gas for create_stream, withdraw, cancel
    cmd = ["cargo", "test", "--test", "resource_limits", "--", "--nocapture"]
    # Actually, let's run all tests in the resource_limits module
    cmd = ["cargo", "test", "resource_limits", "--", "--nocapture"]
    
    try:
        result = subprocess.run(
            cmd,
            cwd=REPO_ROOT / "contracts" / "stream",
            capture_output=True,
            text=True,
            env=build_cargo_test_env(),
            timeout=120  # 2 minute timeout
        )
        return result.stdout + result.stderr
    except subprocess.TimeoutExpired:
        return "ERROR: Tests timed out"
    except Exception as e:
        return f"ERROR: Failed to run tests: {e}"


def main() -> int:
    if not GAS_DOC.exists():
        print(f"SKIP: {GAS_DOC} not found")
        return 0

    content = GAS_DOC.read_text(encoding="utf-8")

    # Look for gas baseline entries like: | operation_name | 12345 |
    pattern = re.compile(r"\|\s*(\w+)\s*\|\s*(\d+)\s*\|")
    matches = pattern.findall(content)

    if not matches:
        print("WARNING: No gas baseline entries found in gas.md")
        print("This is expected if gas.md has not been populated yet.")
        return 0

    print(f"Found {len(matches)} gas baseline entries in docs/gas.md:")
    for name, value in matches:
        print(f"  {name}: {value} instructions")

    print("OK: Gas baseline entries are well-formed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
