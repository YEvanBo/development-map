"""EvanBo / Apache-2.0. Isolated multi-platform bundle acceptance; never opens a user map."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess
import sys
import time
import uuid
from zipfile import ZipFile

args = argparse.ArgumentParser(description=__doc__)
args.add_argument("--work", required=True, type=Path, help="new, nonexistent test directory")
work = args.parse_args().work.resolve()
work.mkdir(parents=True, exist_ok=False)
package = Path(__file__).resolve().parent
runner = json.loads(os.environ.get("DEVMAP_SMOKE_RUNNER", "[]"))
def program(name):
    return runner + [str(package / "bin" / (name + (".exe" if os.name == "nt" else "")))]
db = work / "map.sqlite"
checks = []


def check(name, condition):
    if not condition:
        raise AssertionError(name)
    checks.append(name)


def cli(command, *argv, data=None):
    result = subprocess.run(
        program("mapctl") + [command, str(db), *map(str, argv)],
        input=None if data is None else json.dumps(data, ensure_ascii=False).encode("utf-8"),
        capture_output=True, timeout=15,
    )
    if result.returncode:
        raise RuntimeError(f"mapctl {command} failed: {result.stderr.decode('utf-8', errors='replace')}")
    return json.loads(result.stdout)


def session(token_file, calls, configured_command=None):
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "isolated-bundle-acceptance", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]
    requests.extend({"jsonrpc": "2.0", "id": i + 2, **call} for i, call in enumerate(calls))
    process = subprocess.Popen(
        configured_command or (program("map-mcp") + [str(db), "--token-file", str(token_file)]),
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    try:
        stdout, stderr = process.communicate(
            b"".join(json.dumps(x, ensure_ascii=False).encode("utf-8") + b"\n" for x in requests), timeout=15)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate()
        raise AssertionError("MCP failed to exit after EOF")
    check("MCP exits on EOF", process.returncode == 0)
    check("token absent from output", token_file.read_bytes() not in stdout + stderr)
    responses = [json.loads(line) for line in stdout.splitlines()]
    check("MCP handshake", responses[0]["result"]["protocolVersion"] == "2025-06-18")
    return responses[1:]


def call(name, arguments):
    return {"method": "tools/call", "params": {"name": name, "arguments": arguments}}


def value(response):
    check("MCP call succeeds", not response["result"].get("isError", False))
    return json.loads(response["result"]["content"][0]["text"])


cli("migrate")
cli("project", "acceptance", "Isolated acceptance", "test-fixture")
cli("adopt", "acceptance", "v1", "test-fixture", "1234567890123456", "default", "synthetic-fixture")
tokens = {}
for node in ["first", "other"]:
    cli("node", data={"id": node, "project_id": "acceptance", "parent_id": None,
        "kind": "work_package", "title": node, "goal": "report.csv", "sort_key": node,
        "plan_state": "active", "owner": "test-worker", "generation": 1, "architecture_ref": "test-fixture"})
    token = uuid.uuid4().hex + uuid.uuid4().hex
    tokens[node] = work / f"{node}-token.txt"
    tokens[node].write_text(token, encoding="utf-8")
    if os.name != "nt":
        tokens[node].chmod(0o600)
    cli("bind", data={"token": token, "binding": {"subject": "isolated-test", "project_id": "acceptance",
        "task_id": node, "owner": "test-worker", "role": "worker", "node_id": node, "generation": 1,
        "source_ref": "synthetic-fixture", "expires_at": int(time.time()) + 3600}})

resume = call("map_resume", {"project_id": "acceptance"})
first = session(tokens["first"], [{"method": "tools/list"}, resume])
check("five V1.1 map tools", {x["name"] for x in first[0]["result"]["tools"]} == {"map_resume", "map_project", "map_submit", "map_scope_check", "map_scope_report"})
check("first takeover requests confirmation", value(first[1])["takeover"]["action"] == "confirm_goal")
confirmations = {}
for node in tokens:
    confirmations[node] = cli("confirm-goal", data={"project_id": "acceptance", "node_id": node,
        "generation": 1, "previous": 0, "goal": "Produce report.json with LF", "user_ref": "synthetic-test-confirmation-" + node})["event_id"]
head = value(session(tokens["first"], [resume])[0])
check("latest confirmed goal overrides CSV", head["takeover"]["confirmation"]["goal"] == "Produce report.json with LF")
# Exercise the released V1.1 protocol through fresh stdio processes.
item = {"action":"develop", "project":"acceptance", "resource":"exporter", "scope":"implementation"}
selector = {"project_id":"acceptance", "node_id":"first"}
query = call("map_scope_check", {**selector, "operations":[item]})
check("unregistered scope asks", value(session(tokens["first"], [query])[0])["status"] == "NEEDS_USER_CONFIRMATION")
def decide(approved, revision, reply):
    return call("map_scope_report", {**selector,"goal_revision":confirmations["first"],"scope_revision":revision,
        "approved":approved,"scope":[item],"user_reply":reply,"message_ref":None,"note":"synthetic fixture"})
value(session(tokens["first"], [decide(True,0,"Only exporter")])[0])
check("approved tuple survives restart", value(session(tokens["first"], [query])[0])["status"] == "IN_SCOPE")
extra = call("map_scope_check", {**selector,"operations":[{**item,"resource":"shared"}]})
check("cross-module expansion asks", value(session(tokens["first"], [extra])[0])["status"] == "NEEDS_USER_CONFIRMATION")
snapshot = value(session(tokens["first"], [resume])[0])["task_scope"]
value(session(tokens["first"], [decide(False,snapshot["scope_revision"],"Do not extend")])[0])
after = value(session(tokens["first"], [resume])[0])["task_scope"]
check("refusal advances revision and preserves consent", after["scope_revision"] > snapshot["scope_revision"] and after["user_reply"] == "Only exporter" and after["last_decision_user_reply"] == "Do not extend" and after["scope"] == [item])
other = value(session(tokens["other"], [resume])[0])["task_scope"]
check("another node inherits no scope", not other["scope"])
payload = b'{"ok":true}\n'
(work / "report.json").write_bytes(payload)
checkpoint = {"node_id": "first", "owner": "test-worker", "generation": 1,
    "version": head["checkpoint"]["version"] + 1, "digest": hashlib.sha256(payload).hexdigest(),
    "summary": "Isolated platform test: report.json exact LF bytes verified.", "source_ref": "synthetic-test"}
submission = {"submission_key": "unix-test-delivery", "base_checkpoint_rowid": head["checkpoint"]["rowid"],
    "checkpoint": checkpoint, "artifacts": []}
results = session(tokens["first"], [call("map_submit", {"submission": submission})] * 2)
a, b = map(value, results)
check("exact retry returns original receipt", a["created"] and not b["created"] and a["event_id"] == b["event_id"])
restored = value(session(tokens["first"], [resume])[0])
check("new MCP process recovers committed node", restored["checkpoint"]["rowid"] == a["checkpoint_rowid"])
check("restart does not ask again", restored["takeover"]["action"] == "continue")
cli("revoke-goal", data={"project_id": "acceptance", "node_id": "first",
    "previous": confirmations["first"], "user_ref": "synthetic-test-revocation"})
check("revocation requests fresh confirmation", value(session(tokens["first"], [resume])[0])["takeover"]["action"] == "confirm_goal")
check("other task unaffected", value(session(tokens["other"], [resume])[0])["takeover"]["action"] == "continue")
check("exact JSON bytes retained", (work / "report.json").read_bytes() == payload)
pending = work / ".devmap-pending/test"
pending.mkdir(parents=True)
(pending / "handoff.json").write_text(json.dumps(submission), encoding="utf-8")
snapshot = subprocess.run([sys.executable, str(package / "Skill/scripts/save-pending.py"),
    ".devmap-pending/test", ".devmap-pending/test/handoff.json", "report.json"],
    cwd=work, capture_output=True, timeout=15)
check("pending snapshot survives flush", snapshot.returncode == 0)
with ZipFile(snapshot.stdout.decode().strip()) as z:
    check("pending snapshot preserves exact bytes", z.read("files/report.json") == payload)
cli("backup", work / "backup.sqlite")
rest = subprocess.run(program("mapctl") + ["restore", str(work / "backup.sqlite"), str(work / "restored.sqlite")], capture_output=True, timeout=15)
check("backup restores to new database", rest.returncode == 0 and (work / "restored.sqlite").is_file())
# Read each installed configuration and launch its exact server command twice.
# This checks the config, executable and database together without claiming a host UI test.
try:
    import tomllib
except ImportError:
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(package))
    from vendor import tomli as tomllib
for host in ("codex", "dsh"):
    project = work / ("project " + host)
    project.mkdir()
    setup = [sys.executable, str(package / "install.py")]
    assignment = ["--host", host, "--project", str(project), "--db", str(db),
                  "--token-file", str(tokens["other"])]
    plan = json.loads(subprocess.check_output(setup + ["plan"] + assignment))
    subprocess.run(setup + ["apply"] + assignment + ["--plan-id", plan["plan_id"]],
                   check=True, stdout=subprocess.PIPE)
    contents = Path(plan["configuration"]).read_bytes()
    config = (tomllib.loads(contents.decode())["mcp_servers"]["evanbo_development_map"]
              if host == "codex" else json.loads(contents)[0]["insert"][0]["config"])
    for attempt in range(2):
        recovered = value(session(tokens["other"], [resume], [config["command"], *config["args"]])[0])
        check(host + " configured server resumes assigned goal", recovered["takeover"]["action"] == "continue"
              and recovered["takeover"]["confirmation"]["goal"] == "Produce report.json with LF"
              and recovered["task_scope"]["node_id"] == "other")

report = {"status": "PASS", "platform": platform.platform(), "runner": runner, "checks": len(checks), "assertions": checks,
    "scope": "real platform CLI/MCP processes and isolated synthetic data; not a model-backed Codex/DSH session"}
(work / "result.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(report))
