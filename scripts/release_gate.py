"""Read-only, exact-SHA release prerequisite verification for GitHub Actions."""
import argparse
import json
import re
import subprocess
import sys
import time

WORKFLOWS = ("ci.yml", "msrv.yml", "platform-foundation.yml", "web-driver.yml")
MAX_PAGES = 32


class GateError(Exception):
    """Safe prerequisite refusal; provider bodies and credentials are excluded."""


def api(repository, endpoint, *, deadline=None, **params):
    args = ["gh", "api", "--method", "GET", f"repos/{repository}/{endpoint}"]
    for name, value in params.items():
        args.extend(["-f", f"{name}={value}"])
    try:
        remaining = 30 if deadline is None else min(30, deadline-time.monotonic())
        if remaining <= 0:
            raise GateError("GitHub prerequisite verification exceeded its total deadline")
        result = subprocess.run(args, capture_output=True, timeout=remaining, check=False)
        if result.returncode != 0 or len(result.stdout) > 16 * 1024 * 1024:
            raise GateError("GitHub evidence request failed or exceeded its bound")
        value = json.loads(result.stdout)
        if not isinstance(value, dict):
            raise GateError("GitHub evidence is not an object")
        return value
    except (OSError, subprocess.TimeoutExpired, ValueError) as error:
        raise GateError("GitHub evidence request failed or returned invalid JSON") from error


def inventory(fetch, endpoint, key, **params):
    rows = []
    for page in range(1, MAX_PAGES + 1):
        value = fetch(endpoint, per_page=100, page=page, **params)
        chunk = value.get(key)
        count = value.get("total_count")
        if not isinstance(chunk, list) or not isinstance(count, int) or count < 0:
            raise GateError("GitHub inventory has invalid rows or total_count")
        rows.extend(chunk)
        if len(rows) == count:
            return rows
        if len(rows) > count or not chunk:
            raise GateError("GitHub inventory is incomplete or changed during pagination")
    raise GateError("GitHub inventory exceeded its page bound")


def verify(fetch, commit):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise GateError("Release commit is not an exact SHA")
    admitted = {}
    for workflow in WORKFLOWS:
        runs = inventory(fetch, f"actions/workflows/{workflow}/runs", "workflow_runs",
                         head_sha=commit, event="push", branch="main")
        matching = [run for run in runs if run.get("head_sha") == commit
                    and run.get("head_branch") == "main" and run.get("event") == "push"
                    and run.get("path") == f".github/workflows/{workflow}"]
        if not matching:
            raise GateError(f"{workflow}: missing exact-SHA main-push evidence")
        if any(not isinstance(run.get("id"), int) or not isinstance(run.get("run_attempt"), int)
               or run["id"] <= 0 or run["run_attempt"] <= 0 for run in matching):
            raise GateError(f"{workflow}: invalid run identity")
        run = max(matching, key=lambda row: (row["id"], row["run_attempt"]))
        if run.get("status") != "completed" or run.get("conclusion") != "success":
            raise GateError(f"{workflow}: latest exact-SHA main run is not successful and complete")
        jobs = inventory(fetch, f"actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs", "jobs")
        if not jobs or any(job.get("run_id") != run["id"]
                           or job.get("status") != "completed" or job.get("conclusion") != "success"
                           for job in jobs):
            raise GateError(f"{workflow}: latest attempt jobs are missing or not all successful")
        # Detect a newer attempt/run while jobs were being collected.
        current = fetch(f"actions/runs/{run['id']}")
        if any(current.get(field) != run.get(field) for field in
               ("id", "run_attempt", "head_sha", "head_branch", "event", "status", "conclusion")):
            raise GateError(f"{workflow}: run changed while validating jobs")
        newest = inventory(fetch, f"actions/workflows/{workflow}/runs", "workflow_runs",
                           head_sha=commit, event="push", branch="main")
        if any(row.get("head_sha") == commit and row.get("head_branch") == "main"
               and row.get("event") == "push" and row.get("path") == f".github/workflows/{workflow}"
               and (row.get("id",0), row.get("run_attempt",0)) > (run["id"], run["run_attempt"])
               for row in newest):
            raise GateError(f"{workflow}: newer run appeared during validation")
        admitted[workflow] = run["id"]
    return admitted


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
        raise GateError("Invalid GitHub repository identity")
    deadline = time.monotonic()+120
    runs = verify(lambda endpoint, **params: api(args.repository, endpoint, deadline=deadline, **params), args.commit)
    print(f"driver-run={runs['web-driver.yml']}")


if __name__ == "__main__":
    try:
        main()
    except GateError as error:
        print(f"Release gate refused: {error}", file=sys.stderr)
        sys.exit(1)
