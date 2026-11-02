"""Whole sessions through the Rust program and the Python, on two copies of the state: start, end and
fold on real session logs in sequence, with tool pruning and the facts that need no model call on.
argv: the Rust dam-economizer, a folder holding the `tokeneconomizer` package to compare with."""
import glob, json, os, shutil, subprocess, sys, tempfile

BIN, REF = sys.argv[1], sys.argv[2]
sys.path.insert(0, REF)
from tokeneconomizer.plugin import fold as pyfold, hook as pyhook, repos as pyrepos

pyrepos.start_migration = lambda *a, **k: None
pyrepos.migration_notice = lambda *a, **k: None
pyrepos.adopt = lambda *a, **k: []
pyfold.schedule_pending = lambda *a, **k: None

WORK = tempfile.mkdtemp(prefix='wd', dir=os.environ.get('TEMP'))
CLEAN = {k: v for k, v in os.environ.items() if not k.startswith(('TE_', 'CLAUDE', 'TOKENECONOMIZER'))}
VOLATILE = {'started', 'updated', 'at', 'since', 'ended', 'folded_at', 'last', 'first_seen', 'last_seen', 'transcript'}
SKIP = ('.lock', '.log', '.tmp')


def norm(v):
    if isinstance(v, dict):
        return {k: norm(x) for k, x in v.items() if k not in VOLATILE}
    if isinstance(v, list):
        return [norm(x) for x in v]
    return round(v, 6) if isinstance(v, float) else v


def state(home):
    out = {}
    for path in glob.glob(home + '/repos/*/claude/**', recursive=True):
        if os.path.isfile(path) and not path.endswith(SKIP):
            rel = os.path.relpath(path, home).replace('\\', '/')
            text = open(path, encoding='utf-8').read()
            out[rel] = [norm(json.loads(l)) for l in text.splitlines()] if rel.endswith('.jsonl') else norm(json.loads(text))
    return out


def first_diff(a, b, where=''):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                return f'{where}/{k}: only on one side'
            d = first_diff(a[k], b[k], f'{where}/{k}')
            if d:
                return d
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return f'{where}: {len(a)} vs {len(b)} items'
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f'{where}[{i}]')
            if d:
                return d
        return None
    return None if a == b else f'{where}: {a!r:.200} vs {b!r:.200}'


logs = [p for p in sorted(glob.glob(os.path.expanduser('~') + '/.claude/projects/*/*.jsonl'), key=os.path.getsize) if os.path.getsize(p) > 2000][:int(os.environ.get('MAX', '60'))]
total = bad = 0
for config in ({'microskills': 'envfacts', 'toolprune': 'propose', 'toolprune_min_sessions': 2, 'toolprune_evidence': 2}, {'microskills': 'off', 'toolprune': 'propose'}, {'microskills': 'envfacts', 'toolprune': 'off', 'vocabulary': 'python-git'}):
    run = tempfile.mkdtemp(dir=WORK)
    repo = run + '/repo'
    os.makedirs(repo)
    subprocess.run(['git', 'init', '-q', repo], check=True)
    py_home, rs_home = run + '/py', run + '/rs'
    for home in (py_home, rs_home):
        os.makedirs(home)
        with open(home + '/config.json', 'w') as fh:
            json.dump(config, fh)
    for n, log in enumerate(logs):
        sid = f's{n}'
        for event in ({'hook_event_name': 'SessionStart', 'source': 'startup'}, {'hook_event_name': 'SessionEnd', 'reason': 'exit'}):
            payload = dict(event, session_id=sid, cwd=repo, transcript_path=log)
            env = dict(CLEAN, TE_FOLD_SYNC='1', TE_JUDGE_NO_SPAWN='1', CLAUDECODE='1')
            a = pyhook.run(payload, dict(env, TOKENECONOMIZER_HOME=py_home)) or {}
            r = subprocess.run([BIN, 'hook'], input=json.dumps(payload).encode(), capture_output=True, env=dict(env, TOKENECONOMIZER_HOME=rs_home), cwd=repo)
            b = json.loads(r.stdout or b'{}')
            total += 1
            problem = first_diff(a, b, 'answer ' + event['hook_event_name']) or first_diff(state(py_home), state(rs_home), 'state')
            if problem:
                bad += 1
                print(config, sid, problem[:400])
                break
        else:
            continue
        break
print(f'{len(logs)} logs, {total} hooks, configurations stopped by a difference: {bad}')
shutil.rmtree(WORK, ignore_errors=True)
