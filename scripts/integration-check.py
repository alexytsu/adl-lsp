#!/usr/bin/env python3
"""Integration check: run adl-lsp against a real workspace.

For every .adl file under the search dirs: open it, record diagnostics, then hover and
goto-definition on a usage of every imported type. Reports error responses, unresolved
definitions and diagnostics.

usage: scripts/integration-check.py <adl-lsp binary> <workspace> [search dir relative to workspace ...]

The search dir defaults to the workspace itself, as in the extension. Set VERBOSE=1 to list
every problem. Exits non-zero if any request failed, any import was unresolved or any
diagnostic was reported.
"""
import json, os, pathlib, re, subprocess, sys, collections, tempfile

binary, workspace, *search = sys.argv[1:]
workspace = pathlib.Path(workspace).resolve()
search_dirs = [str((workspace / s).resolve()) for s in (search or ["."])]
verbose = os.environ.get("VERBOSE")

files = sorted(p for d in search_dirs for p in pathlib.Path(d).rglob("*.adl")
               if "node_modules" not in p.parts and "target" not in p.parts)

log = tempfile.NamedTemporaryFile("w", prefix="adl-lsp-", suffix=".log", delete=False)
p = subprocess.Popen([binary, "--client", "vscode", "--search-dirs", ",".join(search_dirs)],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log)
diagnostics = {}

def send(msg):
    body = json.dumps(msg).encode()
    p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    p.stdin.flush()

def recv():
    length = None
    while True:
        line = p.stdout.readline()
        if not line:
            raise SystemExit("FATAL: server closed stdout (crashed)")
        line = line.strip()
        if not line:
            break
        k, v = line.split(b":", 1)
        if k.lower() == b"content-length":
            length = int(v)
    return json.loads(p.stdout.read(length))

next_id = [0]
def request(method, params):
    next_id[0] += 1
    send({"jsonrpc": "2.0", "id": next_id[0], "method": method, "params": params})
    while True:
        msg = recv()
        if msg.get("id") == next_id[0] and "method" not in msg:
            return msg
        if msg.get("method") == "textDocument/publishDiagnostics":
            diagnostics[msg["params"]["uri"]] = msg["params"]["diagnostics"]

init = request("initialize", {"processId": None, "rootUri": workspace.as_uri(), "capabilities": {}})
version = init["result"]["serverInfo"]["version"]
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
scan_diagnostics = dict(diagnostics)

stats = collections.Counter()
problems = []
IMPORT = re.compile(r"^\s*import\s+([\w.]+)\.(\w+)\s*;")
for path in files:
    text = path.read_text()
    uri = path.as_uri()
    rel = path.relative_to(workspace)
    diagnostics.pop(uri, None)
    send({"jsonrpc": "2.0", "method": "textDocument/didOpen",
          "params": {"textDocument": {"uri": uri, "languageId": "adl", "version": 1, "text": text}}})
    lines = text.splitlines()
    imports = [m.groups() for m in map(IMPORT.match, lines) if m]
    for module, name in imports:
        usage = None
        for i, line in enumerate(lines):
            if IMPORT.match(line) or line.strip().startswith("//"):
                continue
            code = line.split("//")[0]
            m = re.search(r"(?<![\w.\"])%s\b(?!\")" % re.escape(name), code)
            if m:
                usage = {"line": i, "character": m.start() + 1}
                break
        if usage is None:
            stats["imports never used (skipped)"] += 1
            continue
        stats["imported symbols checked"] += 1
        if module.startswith("sys.") or module.startswith("adlc."):
            stats["  of which stdlib (sys.*/adlc.*)"] += 1
        where = f"{rel}:{usage['line'] + 1} {module}.{name}"
        tdp = {"textDocument": {"uri": uri}, "position": usage}
        for method in ("textDocument/hover", "textDocument/definition"):
            r = request(method, tdp)
            short = method.split("/")[1]
            if "error" in r:
                stats[f"{short}: ERROR response"] += 1
                problems.append(f"{short} error   {where}: {r['error']['message']}")
            elif short == "definition":
                result = r["result"]
                if not result:
                    stats["definition: unresolved (null)"] += 1
                    problems.append(f"definition null  {where}")
                else:
                    target = pathlib.Path(result["uri"].replace("file://", ""))
                    expected = module.replace(".", "/") + ".adl"
                    if str(target).endswith(expected):
                        stats["definition: resolved"] += 1
                    else:
                        stats["definition: WRONG FILE"] += 1
                        problems.append(f"definition wrong {where} -> {target}")
            else:
                items = (r["result"] or {}).get("contents") or []
                stats["hover: with content" if items else "hover: empty"] += 1
                if not items:
                    problems.append(f"hover empty      {where}")
    request("textDocument/documentSymbol", {"textDocument": {"uri": uri}})  # flush diagnostics
    for d in diagnostics.get(uri, []):
        stats["diagnostics (open files)"] += 1
        problems.append(f"diagnostic       {rel}:{d['range']['start']['line'] + 1}: {d['message']}")

for uri, ds in scan_diagnostics.items():
    for d in ds:
        if "import" in d["message"]:
            stats["import diagnostics from initial workspace scan"] += 1
            problems.append(f"scan diagnostic  {uri.replace(workspace.as_uri() + '/', '')}:{d['range']['start']['line'] + 1}: {d['message']}")

alive = p.poll() is None
request("shutdown", None)
send({"jsonrpc": "2.0", "method": "exit"})
p.wait(timeout=5)
log.close()
errors = sum(1 for l in open(log.name) if " ERROR " in l)

print(f"workspace {workspace}  server {version}  files {len(files)}  search dirs {[os.path.relpath(s, workspace) for s in search_dirs]}")
for k in sorted(stats):
    print(f"  {k:48} {stats[k]}")
print(f"  {'server ERROR log lines':48} {errors}")
print(f"  {'server alive at end':48} {alive}")
print(f"  {'server log':48} {log.name}")
shown = problems if verbose else problems[:8]
for line in shown:
    print("    " + line)
if len(problems) > len(shown):
    print(f"    ... {len(problems) - len(shown)} more (VERBOSE=1 for all)")
bad = [k for k in stats if "ERROR" in k or "WRONG" in k or "null" in k or "diagnostic" in k]
sys.exit(1 if bad or not alive else 0)
