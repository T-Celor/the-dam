"""The Rust loop guard against the Python one on the same calls: real sessions' tool calls and made-up loops."""
import glob, json, os, random, subprocess, sys

BIN = sys.argv[1]
sys.path.insert(0, os.path.expanduser('~') + '/TokenEconomizer')
from tokeneconomizer import loopguard as lg
from tokeneconomizer.loopguard.envelope import EnvelopeTracker, load_envelope, thresholds
from tokeneconomizer.loopguard.shell_edits import shell_edit_targets
from tokeneconomizer.loopguard.turn import TurnGuard, VERIFY_RE, status_of_tests


class Store:
    def __init__(self, saved):
        self.saved = saved

    def read(self, name):
        return self.saved


def scripted(turns):
    if turns % 4 == 0:
        return {'stuck': True, 'confidence': 0.9, 'reason': f'scripted {turns}', '_tokens': 11}
    if turns % 4 == 1:
        return {'stuck': False, 'confidence': 0.2, 'reason': f'scripted {turns}', '_tokens': 7}
    if turns % 4 == 2:
        return {'stuck': None, 'confidence': 0.0, 'reason': f'scripted {turns}', '_tokens': 3}
    return None


def python_case(case):
    if 'command' in case:
        c = case['command']
        return {'targets': shell_edit_targets(c), 'status': status_of_tests(c), 'verifies': bool(VERIFY_RE.search(c))}
    ov = case.get('overrides') or {}
    level = case.get('level', 'default')
    tracker = load_envelope(Store(case.get('saved') or {}), level=level, overrides=ov)
    envelope = tracker.envelope
    thr = thresholds(tracker, ov)
    learned = dict(thr)
    if case.get('plugin'):
        thr = dict(thr, gate_over_envelope=False, gate_min_turns=max(thr['gate_min_turns'], thr['min_turns']))
    guard = TurnGuard(turn_key='t')
    decisions, digests = [], []
    for call in case.get('calls') or []:
        guard = TurnGuard(data=json.loads(json.dumps(guard.to_dict())))
        turns_next = guard.st.turns + 1

        def judge(digest, t=turns_next):
            digests.append(digest)
            return scripted(t)
        d = guard.fold(call.get('name'), call.get('args'), call.get('result'), call.get('code'), envelope=envelope, thr=thr,
                       judge=judge if case.get('judge') else None, nudge_budget=case.get('budget', 1))
        body = None
        if d and d[0] == 'nudge':
            body = guard.nudge_body(envelope)
            guard.after_nudge()
        decisions.append({'decision': list(d) if d else None, 'body': body})
    tracker.update(guard.cum_turns, guard.resolved_proxy(True), snapshot=guard.whole.snapshot())
    return {'decisions': decisions, 'digests': digests, 'guard': guard.to_dict(), 'thr': learned, 'envelope': envelope, 'summary': guard.summary(True),
            'saved_after': {'level': tracker.level, 'resolved_turns': tracker.resolved_turns[-500:], 'resolved_snaps': tracker.resolved_snaps[-500:]},
            'digest': guard.st.recent_digest(envelope=envelope), 'state': guard.verification_state()}


def same(a, b, where=''):
    if isinstance(a, dict) and isinstance(b, dict):
        if set(a) != set(b):
            return f'{where}: keys {sorted(set(a) ^ set(b))}'
        for k in a:
            r = same(a[k], b[k], where + '/' + k)
            if r:
                return r
        return None
    if isinstance(a, (list, tuple)) and isinstance(b, (list, tuple)):
        if len(a) != len(b):
            return f'{where}: {len(a)} vs {len(b)} items'
        for i, (x, y) in enumerate(zip(a, b)):
            r = same(x, y, f'{where}[{i}]')
            if r:
                return r
        return None
    if isinstance(a, (int, float)) and isinstance(b, (int, float)) and not isinstance(a, bool):
        return None if abs(a - b) < 1e-9 else f'{where}: {a} vs {b}'
    return None if a == b else f'{where}: {a!r:.300} vs {b!r:.300}'


# Real tool calls from this machine's sessions: nothing leaves it.
sessions = []
commands = set()
for path in sorted(glob.glob(os.path.expanduser('~') + '/.claude/projects/*/*.jsonl'))[:int(os.environ.get('MAX_SESSIONS', '120'))]:
    pend, calls = {}, []
    for line in open(path, encoding='utf-8', errors='ignore'):
        try:
            ev = json.loads(line)
        except Exception:
            continue
        content = (ev.get('message') or {}).get('content') if isinstance(ev.get('message'), dict) else None
        if not isinstance(content, list):
            continue
        for b in content:
            if not isinstance(b, dict):
                continue
            if b.get('type') == 'tool_use':
                pend[b.get('id')] = (b.get('name'), b.get('input') or {})
            elif b.get('type') == 'tool_result' and b.get('tool_use_id') in pend:
                name, args = pend.pop(b['tool_use_id'])
                c = b.get('content')
                if isinstance(c, list):
                    c = ' '.join(x.get('text', '') for x in c if isinstance(x, dict))
                calls.append({'name': name, 'args': args, 'result': (c if isinstance(c, str) else '')[:40000], 'code': 1 if b.get('is_error') else None})
                if name == 'Bash' and isinstance(args.get('command'), str):
                    commands.add(args['command'])
    if calls:
        sessions.append(calls)

random.seed(7)
ERRS = ['E  ValueError: bad value 12 at /a/b.py', 'FAILED tests/test_a.py::t - assert 1 == 2', 'Traceback (most recent call last):\n  x\nKeyError: 3', '3 failed, 2 passed in 1.20s', 'not ok 4 - thing', '--- FAIL: TestX', 'All good', '12 passed in 0.5s', 'test result: FAILED. 1 passed', 'OK', 'exception thrown', 'é ünï Error']
CMDS = ['pytest -q', 'pytest -q 2>&1 | tail -n 8', 'npm test || true', 'python -m unittest; echo "exit=$?"', 'pytest -q; git diff', "sed -i 's/a/b/' src/a.py && pytest", 'cat <<EOF > x.py\nprint(1)\nEOF', 'go test ./...', 'cargo test', 'ls -la', 'echo "unbalanced', 'bash -lc \'pytest -q | tail\'', 'FOO=1 timeout 60 npm test', 'perl -pi -e "s/a/b/" a.c b.c', 'tee a.md < x', "python - <<'P'\nfrom pathlib import Path\np = Path('z.py')\np.write_text('x')\nP", "git checkout -- a.py b.py", 'apply_patch <<\'X\'\n*** Update File: src/q.rs\nX', 'pytest --help', 'make test | grep FAIL', '# comment > a.py\nls # x > b.py', 'a=1 b=2 ./bin/test', 'echo a>b.py;echo c>>d.txt', 'cmd 2>err.py', '(cd x && pytest)', 'pytest>|out.py']
TOOLS = [('Edit', lambda: {'file_path': random.choice(['a/a.py', 'b.py', 'c/d/e.ts', 'x.rs'])}), ('Write', lambda: {'file_path': 'w/new.py'}), ('Read', lambda: {'file_path': 'r.py'}), ('Grep', lambda: {'pattern': 'x' * random.randint(1, 60)}), ('Glob', lambda: {'pattern': '*.py'}), ('MultiEdit', lambda: {'file_path': 'a/a.py'}), ('Task', lambda: {})]
synthetic = []
for n in range(400):
    calls = []
    errs = random.sample(ERRS, random.randint(1, 4))
    for _ in range(random.randint(1, 90)):
        if random.random() < 0.55:
            name, args = 'Bash', {'command': random.choice(CMDS)}
        else:
            name, make = random.choice(TOOLS)
            args = make()
        calls.append({'name': name, 'args': args, 'result': random.choice(errs + ['', 'fine']), 'code': random.choice([None, None, 0, 1])})
    synthetic.append(calls)


def saved(r):
    n = r.randint(0, 12)
    turns = [r.randint(1, 60) for _ in range(n)]
    return {'resolved_turns': turns, 'resolved_snaps': [{'edit_churn': r.randint(0, 9), 'distinct_edit_files': r.randint(0, 6), 'error_outputs': r.randint(0, 12), 'stagnation': round(r.random(), 3)} for _ in range(n if r.random() < 0.7 else 0)]}


cases = [{'command': c} for c in sorted(commands)] + [{'command': c} for c in CMDS]
r = random.Random(11)
for calls in sessions + synthetic:
    cases.append({'calls': calls, 'level': r.choice(['default', 'aggressive', 'conservative']), 'saved': saved(r), 'plugin': r.random() < 0.6, 'judge': r.random() < 0.5,
                  'budget': r.choice([0, 1, 1, 2]), 'overrides': r.choice([{}, {}, {'backstop_mult': 2.0}, {'min_turns': 5, 'envelope_min_samples': 2}])})
print(f'{len(sessions)} real sessions, {sum(map(len, sessions))} real calls, {len(commands)} real commands, {len(synthetic)} made-up loops, {len(cases)} cases')
text = '\n'.join(json.dumps(c) for c in cases) + '\n'
run = subprocess.run([BIN, 'replay'], input=text.encode(), capture_output=True)
if run.returncode:
    print('rust failed', run.stderr.decode(errors='replace')[-800:])
results = run.stdout.decode().splitlines()
print('results', len(results))
bad = 0
acted = 0
kinds = {}
for case, line in zip(cases, results):
    want = python_case(case)
    got = json.loads(line)
    problem = same(want, got)
    if 'decisions' in want:
        acted += sum(1 for d in want['decisions'] if d['decision'])
    if problem:
        bad += 1
        key = problem.split(':')[0].split('[')[0][:40]
        kinds.setdefault(key, []).append((problem, case.get('command', '')[:200]))
print(f'decisions taken by the guard in these cases: {acted}')
print(f'differences: {bad} of {len(results)}')
for key, found in sorted(kinds.items(), key=lambda kv: -len(kv[1]))[:12]:
    print(' ', key, len(found))
    for problem, command in found[:3]:
        print('    ', problem[:400], '| CMD:', command[:160].replace('\n', '\\n'))
