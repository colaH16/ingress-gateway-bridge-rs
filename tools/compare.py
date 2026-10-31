#!/usr/bin/env python3
"""Compile a temporary Go adapter against an untouched source checkout; compare JSON."""
import argparse
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]

def normalized(result):
    # Go's API structs emit these empty server-managed fields; Rust omits them.
    for route in result.get("routes", []):
        if route.get("status") == {"parents": None} or route.get("status") == {}:
            route.pop("status", None)
        if route["metadata"].get("creationTimestamp") is None:
            route["metadata"].pop("creationTimestamp", None)
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go-source", type=pathlib.Path, default=ROOT.parent / "worktrees/ingress-gateway-bridge/independent-review-20261019")
    args = parser.parse_args()
    source = args.go_source.resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    subprocess.run(["cargo", "build", "--locked"], cwd=ROOT, check=True)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version=1"], cwd=ROOT))
    binary = pathlib.Path(metadata["target_directory"]) / "debug" / "ingress-gateway-bridge-rs"
    with tempfile.TemporaryDirectory(prefix="bridge-oracle-") as temp:
        temp = pathlib.Path(temp)
        module = "github.com/colaH16/ingress-gateway-bridge"
        (temp / "go.mod").write_text(f'module {module}/comparison\n\ngo 1.25.0\n\nrequire {module} v0.0.0\nreplace {module} => {json.dumps(str(source))}\n')
        (temp / "main.go").write_text((ROOT / "tools/oracle.go").read_text())
        subprocess.run(["go", "mod", "tidy"], cwd=temp, check=True)
        oracle = temp / "oracle"
        subprocess.run(["go", "build", "-o", str(oracle), "."], cwd=temp, check=True)
        cases = json.loads((ROOT / "tests/cases.json").read_text())
        for case in cases:
            data = json.dumps(case["input"])
            go = subprocess.run([str(oracle)], input=data, text=True, capture_output=True, check=True)
            rust = subprocess.run([str(binary)], input=data, text=True, capture_output=True)
            expected_code = {"ok": 0, "rejected": 2, "dependency": 3}[case["expected"]]
            if rust.returncode != expected_code:
                raise AssertionError(f'{case["name"]}: exit {rust.returncode}: {rust.stderr}')
            actual, expected = normalized(json.loads(rust.stdout)), normalized(json.loads(go.stdout))
            if actual != expected:
                raise AssertionError(f'{case["name"]}\nRust: {actual}\nGo: {expected}')
        print(f"PASS: {len(cases)} shared scenarios; Go revision {revision}")

if __name__ == "__main__":
    main()
