"""The Rust hooks against the Python ones: the same hook payloads on two copies of the same state."""
import glob, io, json, os, random, shutil, statistics, subprocess, sys, tempfile, time

BIN = sys.argv[1]
TE = os.path.expanduser('~') + '/TokenEconomizer'
sys.path.insert(0, TE)
from tokeneconomizer.plugin import hook as pyhook, judge as pyjudge, paths, repos
from tokeneconomizer.plugin.hosts import ClaudeCode

pyjudge.spawn_job = lambda *a, **k: None   # no model call, on either side
TEMP = os.environ.get('TEMP', '/tmp')
WORK = tempfile.mkdtemp(prefix='hd', dir=TEMP)
PROJECT = WORK + '/project'
os.makedirs(PROJECT)
CLEAN = {k: v for k, v in os.environ.items() if not k.startswith(('TE_', 'CLAUDE_', 'TOKENECONOMIZER'))}


def run_py(home, payload):
    env = dict(CLEAN, TOKENECONOMIZER_HOME=home)
    return pyhook.run(payload, env) or {}


def run_rust(home, payload):
    env = dict(CLEAN, TOKENECONOMIZER_HOME=home, TE_JUDGE_NO_SPAWN='1')
    r = subprocess.run([BIN, 'hook'], input=json.dumps(payload).encode(), capture_output=True, env=env, cwd=PROJECT)
    if r.returncode:
        raise SystemExit(f'rust exit {r.returncode}: {r.stderr[-400:]}')
    return json.loads(r.stdout or b'{}')


VOLATILE = {'started', 'updated', 'at', 'since'}


def norm(v, key=''):
    if isinstance(v, dict):
        return {k: norm(x, k) for k, x in v.items() if k not in VOLATILE}
    if isinstance(v, list):
        return [norm(x) for x in v]
    if key == 'id' and isinstance(v, str):
        return v.rsplit(':', 1)[0]
    return v


def same(a, b, where=''):
    if isinstance(a, dict) and isinstance(b, dict):
        if set(a) != set(b):
            return f'{where}: keys {sorted(set(a) ^ set(b))}'
        for k in a:
            r = same(a[k], b[k], where + '/' + k)
            if r:
                return r
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return f'{where}: {len(a)} vs {len(b)}'
        for i, (x, y) in enumerate(zip(a, b)):
            r = same(x, y, f'{where}[{i}]')
            if r:
                return r
        return None
    if isinstance(a, (int, float)) and isinstance(b, (int, float)) and not isinstance(a, bool):
        return None if abs(a - b) < 1e-6 else f'{where}: {a} vs {b}'
    return None if a == b else f'{where}: {a!r:.260} vs {b!r:.260}'


def state_of(home):
    out = {}
    for path in glob.glob(home + '/repos/*/claude/**', recursive=True):
        if os.path.isfile(path) and not path.endswith(('.lock', '.log')):
            rel = os.path.relpath(path, home).replace('\\', '/')
            text = open(path, encoding='utf-8').read()
            if rel.endswith('.jsonl'):
                out[rel] = [norm(json.loads(l)) for l in text.splitlines()]
            elif rel.endswith('.json'):
                out[rel] = norm(json.loads(text))
    return out


def transcript_payloads(sid):
    """Hook payloads rebuilt from real session logs: the tool_response each tool really returned."""
    streams = []
    for path in sorted(glob.glob(os.path.expanduser('~') + '/.claude/projects/*/*.jsonl'))[:int(os.environ.get('MAX_REAL', '25'))]:
        pend, events, turn = {}, [], 0
        for line in open(path, encoding='utf-8', errors='ignore'):
            try:
                ev = json.loads(line)
            except Exception:
                continue
            msg = ev.get('message') if isinstance(ev.get('message'), dict) else None
            if not msg:
                continue
            content = msg.get('content')
            if ev.get('type') == 'user' and (isinstance(content, str) or (isinstance(content, list) and any(isinstance(b, dict) and b.get('type') == 'text' for b in content))):
                turn += 1
                if events:
                    events.append({'hook_event_name': 'Stop'})
                events.append({'hook_event_name': 'UserPromptSubmit', 'prompt': 'x', 'prompt_id': f'p{turn}'})
            if not isinstance(content, list):
                continue
            for b in content:
                if not isinstance(b, dict):
                    continue
                if b.get('type') == 'tool_use':
                    pend[b.get('id')] = (b.get('name'), b.get('input') or {})
                elif b.get('type') == 'tool_result' and b.get('tool_use_id') in pend:
                    name, args = pend.pop(b['tool_use_id'])
                    base = {'tool_name': name, 'tool_input': args, 'prompt_id': f'p{turn}'}
                    if b.get('is_error'):
                        c = b.get('content')
                        c = c if isinstance(c, str) else ' '.join(x.get('text', '') for x in (c or []) if isinstance(x, dict))
                        events.append(dict(base, hook_event_name='PostToolUseFailure', error=c[:30000]))
                    else:
                        resp = ev.get('toolUseResult')
                        if resp is None:
                            resp = b.get('content')
                        if isinstance(resp, str):
                            resp = resp[:30000]
                        elif len(json.dumps(resp)) > 60000:
                            resp = {'stdout': json.dumps(resp)[:30000]}
                        events.append(dict(base, hook_event_name='PostToolUse', tool_response=resp))
        if events:
            streams.append(events[:150])
    return streams


def loop_payloads(r):
    """A made-up session of turns that loop, to reach redirects, notices and the judge."""
    ev, ERRS = [], ['E ValueError: bad 12', 'FAILED t.py::x - assert 1', 'Traceback (most recent call last):\n KeyError', 'ok', '3 failed, 1 passed in 2.0s']
    for turn in range(r.randint(1, 4)):
        pid = f'q{turn}' if r.random() < 0.9 else ''
        if r.random() < 0.8:
            ev.append({'hook_event_name': 'UserPromptSubmit', 'prompt': 'go', 'prompt_id': pid})
        same_err = r.choice(ERRS)
        for _ in range(r.randint(1, 45)):
            kind = r.random()
            if kind < 0.4:
                ev.append({'hook_event_name': 'PostToolUse', 'tool_name': 'Edit', 'tool_input': {'file_path': r.choice(['/a/x.py', '/a/y.py'])}, 'tool_response': {'filePath': '/a/x.py', 'é': 1}, 'prompt_id': pid})
            elif kind < 0.8:
                resp = {'stdout': same_err if r.random() < 0.8 else r.choice(ERRS), 'stderr': '', 'interrupted': False}
                ev.append({'hook_event_name': 'PostToolUse', 'tool_name': 'Bash', 'tool_input': {'command': r.choice(['pytest -q', 'pytest | tail', 'npm test', "sed -i 's/a/b/' z.py"])}, 'tool_response': resp, 'prompt_id': pid})
            elif kind < 0.95:
                ev.append({'hook_event_name': 'PostToolUseFailure', 'tool_name': 'Bash', 'tool_input': {'command': 'pytest'}, 'error': 'Exit code 1\n' + same_err, 'prompt_id': pid})
            else:
                ev.append({'hook_event_name': 'PostToolUse', 'tool_name': 'Task', 'tool_input': {}, 'tool_response': [{'text': 'sub'}], 'agent_id': 'a1' if r.random() < 0.5 else '', 'prompt_id': pid})
        if r.random() < 0.8:
            ev.append({'hook_event_name': 'Stop', 'stop_hook_active': r.random() < 0.1, 'prompt_id': pid})
    return ev


r = random.Random(5)
streams = transcript_payloads('s')
real = len(streams)
streams += [loop_payloads(r) for _ in range(120)]
bad, events, outs, kinds = 0, 0, 0, {}
py_times, rust_times = [], []
for n, stream in enumerate(streams):
    sid = f'sess{n}'
    home_py = f'{WORK}/h{n}p'
    transcript = f'{WORK}/{sid}.jsonl'
    common = {'session_id': sid, 'cwd': PROJECT, 'transcript_path': transcript}
    # Pin as a real session start does.
    os.makedirs(home_py, exist_ok=True)
    repo = paths.repo_identity(PROJECT, dict(CLEAN, TOKENECONOMIZER_HOME=home_py))
    repos.pin(home_py, 'claude', sid, repo, PROJECT)
    base = paths.repo_state_dir(repo, 'claude', dict(CLEAN, TOKENECONOMIZER_HOME=home_py))
    os.makedirs(base)
    if n >= real or n % 2:
        turns = [r.randint(2, 40) for _ in range(r.randint(0, 12))]
        json.dump({'resolved_turns': turns, 'resolved_snaps': [{'edit_churn': r.randint(0, 4), 'distinct_edit_files': r.randint(0, 2), 'error_outputs': r.randint(0, 5), 'stagnation': 0.1} for _ in turns]}, open(base + '/envelope.json', 'w'))
    cfg = r.choice([{}, {}, {}, {'loopguard': 'off'}, {'spiral_nudges': 2}, {'loopguard_aggr': 'aggressive', 'shadow': True}, {'loopguard.min_turns': 6, 'mode': 'headless'}])
    json.dump(cfg, open(os.path.dirname(base) + '/config.json', 'w'))
    if r.random() < 0.3:
        os.makedirs(home_py + '/hosts/claude')
        json.dump({'at': 1, 'reason': 'x'}, open(home_py + '/hosts/claude/sidecall.json', 'w'))
    home_rs = f'{WORK}/h{n}r'
    shutil.copytree(home_py, home_rs)
    for i, ev in enumerate(stream):
        payload = dict(common, **ev)
        t = time.perf_counter()
        a = run_py(home_py, payload)
        py_times.append(time.perf_counter() - t)
        t = time.perf_counter()
        b = run_rust(home_rs, payload)
        rust_times.append(time.perf_counter() - t)
        events += 1
        outs += bool(a)
        # A verdict for whatever job each side has out, so the judge's path runs too.
        for home in (home_py, home_rs):
            session_file = f'{paths.repo_state_dir(repo, "claude", dict(CLEAN, TOKENECONOMIZER_HOME=home))}/sessions/{sid}.json'
            if not os.path.exists(session_file):
                continue
            doc = json.load(open(session_file))
            pending = (doc.get('turn') or {}).get('judge_pending')
            if pending:
                seq = int(pending['id'].split(':')[2])
                verdict = [{'stuck': True, 'confidence': 0.95, 'reason': 'loop', '_nudge': ' written redirect ', '_tokens': 5}, {'stuck': False, 'confidence': 0.3}, None, {'stuck': None, 'confidence': 0}][seq % 4]
                judge_dir = paths.repo_state_dir(repo, 'claude', dict(CLEAN, TOKENECONOMIZER_HOME=home)) + '/judge'
                os.makedirs(judge_dir, exist_ok=True)
                json.dump({'id': pending['id'], 'verdict': verdict, 'at': 1}, open(f'{judge_dir}/{sid}.result.json', 'w'))
        problem = same(norm(a), norm(b), f'output {i} {ev["hook_event_name"]}') or same(state_of(home_py), state_of(home_rs), f'state after {i} {ev["hook_event_name"]}')
        if problem:
            bad += 1
            key = problem.split(':')[0].split('[')[0][:60]
            kinds.setdefault(key, []).append(problem)
            break
print(f'{real} real sessions, {len(streams) - real} made-up loops, {events} hooks, {outs} visible answers; streams with a difference: {bad}')
for key, found in sorted(kinds.items(), key=lambda kv: -len(kv[1]))[:12]:
    print(' ', len(found), found[0][:520])
print(f'python in-process median {statistics.median(py_times) * 1000:.1f} ms (a real hook adds the interpreter start); rust with process start median {statistics.median(rust_times) * 1000:.1f} ms')
shutil.rmtree(WORK, ignore_errors=True)
