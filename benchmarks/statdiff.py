# Rust `status` against the Python's, on copies of the state: Python's status may migrate state.
import json, os, shutil, subprocess, sys, tempfile
HOME = os.path.expanduser('~'); T = os.environ['TEMP']
PLUG = sys.argv[1]; RS = sys.argv[2]; MUT = len(sys.argv) > 3
BOOT = PLUG + '/scripts/te_bootstrap.py'
work = tempfile.mkdtemp(dir=T, prefix='sd')


def env(state, **kw):
    e = {k: v for k, v in os.environ.items() if not k.startswith(('TE_', 'CLAUDE_CODE_SESSION'))}
    e.update(TOKENECONOMIZER_HOME=state, CLAUDECODE='1', PYTHONUTF8='1', CLAUDE_PLUGIN_ROOT=PLUG)
    e.update(kw)
    return e


def run(argv, e):
    r = subprocess.run(argv, env=e, capture_output=True, encoding='utf-8', errors='replace')
    return r.returncode, r.stdout


def cmp(name, cwd, state, **kw):
    bad = 0
    for flags, parse in ((['--json'], True), ([], False)):
        rr, ro = run([RS, '--cwd', cwd, 'status', *flags], env(state, **kw))
        pr, po = run([sys.executable, BOOT, 'cli', '--cwd', cwd, 'status', *flags], env(state, **kw))
        if MUT and parse:
            ro = ro.replace('"nudges": ', '"nudges": 1')
        if parse:
            try:
                a, b = json.loads(ro), json.loads(po)
            except ValueError:
                print('UNPARSED', name, rr, ro[:200], pr, po[:200]); bad += 1; continue
            if a != b:
                bad += 1
                for k in sorted(set(a) | set(b)):
                    if a.get(k) != b.get(k):
                        print('DIFF', name, k, '\n  rs', a.get(k), '\n  py', b.get(k))
            elif ro.strip() != po.strip():
                print('TEXTJSON', name); bad += 1
        elif ro.strip() != po.strip():
            bad += 1; print('TEXT', name)
            for x, y in zip(ro.splitlines(), po.splitlines()):
                if x != y:
                    print('  rs', x, '\n  py', y)
    return bad


n = bad = 0
# Real state, real repositories.
real = os.path.join(work, 'real')
if os.path.isdir(HOME + '/.tokeneconomizer'):
    shutil.copytree(HOME + '/.tokeneconomizer', real, ignore=shutil.ignore_patterns('bin'))
for d in [HOME + '/the-dam', HOME + '/TokenEconomizer', HOME + '/TokenEconomizer/tokeneconomizer', T + '/dg/TokenEconomizer', T + '/dg/headroom', T + '/dg/Optimism', T + '/dg/Rgr', HOME, T, PLUG]:
    if os.path.isdir(d):
        n += 1; bad += cmp('real:' + d, d, real)
# Made-up repositories.
syn = os.path.join(work, 'syn'); os.makedirs(syn)
R = os.path.join(work, 'repos'); os.makedirs(R)


def g(*a, cwd=None):
    subprocess.run(['git', *a], cwd=cwd, capture_output=True)


for name, url in [('plain', None), ('ssh', 'git@github.com:Owner/Repo.git'), ('https', 'https://github.com/Owner/Repo/'), ('Weird Name é', None), ('scp', 'host.example:team/x.git'), ('port', 'ssh://git@h:2222/a/b.git')]:
    d = os.path.join(R, name); os.makedirs(d); g('init', '-q', d)
    if url:
        g('remote', 'add', 'origin', url, cwd=d)
    os.makedirs(d + '/sub/dir')
    n += 2; bad += cmp('repo:' + name, d, syn) + cmp('sub:' + name, d + '/sub/dir', syn)
d = os.path.join(R, 'plain')
g('-c', 'user.name=a', '-c', 'user.email=a@a', 'commit', '-q', '--allow-empty', '-m', 'x', cwd=d)
g('worktree', 'add', '-q', os.path.join(R, 'wt'), cwd=d)
n += 1; bad += cmp('worktree', os.path.join(R, 'wt'), syn)
os.makedirs(R + '/emptygit/.git'); n += 1; bad += cmp('emptygit', R + '/emptygit', syn)
os.makedirs(R + '/nogit'); n += 1; bad += cmp('nogit', R + '/nogit', syn)
# Settings: each accepted, refused and odd value.
base = R + '/nogit'
vals = [{'loopguard': 'guard'}, {'loopguard': True}, {'loopguard': 'banana'}, {'spiral': 'off', 'loopguard': 'nudge'}, {'mode': 'AUTO'}, {'mode': 'headless'}, {'microskills': 'on'}, {'inject': 'envfacts'}, {'toolprune': 0}, {'untool': 'propose'}, {'toolprune': 1}, {'repomap': 'on'}, {'repomap': 'start'}, {'shadow': 'yes'}, {'shadow': 1}, {'enabled': 'no'}, {'vocabulary': 'node-web'}, {'vocabulary': 'x'}, {'paid': True}, {'nonsense': 1}, {'loopguard_nudges': 0}, {'loopguard_nudges': '3'}, {'loopguard_nudges': 'x'}, {'loopguard_nudges': 2.7}, {'loopguard_nudges': None}, {'toolprune_quiet_minutes': 0}, {'toolprune_quiet_minutes': -1}, {'toolprune_protect': 'a, b,,c'}, {'toolprune_protect': ['x', ' y ', 1]}, {'repomap_tokens': [1]}, {'repomap_python': ' p '}, {'paid_model': 'm'}, {'loopguard_thresholds': 5}, {'loopguard_thresholds': {'min_turns': 7, 'stagnation': 2.0, 'bad': 1}}, {'spiral_thresholds': {'edit_churn': '3'}}, {'loopguard.min_turns': '3.5'}, {'loopguard.min_turns': 3.5}, {'loopguard.min_turns': 0}, {'loopguard.min_turns': True}, {'loopguard.stagnation': 'abc'}, {'loopguard.stagnation': 0.5}, {'loopguard.envelope_pct': 101}, {'loopguard.envelope_margin': 0.05}, {'spiral.backstop_mult': 2}, {'loopguard.judge_confidence': None}, {'loopguard.envelope_min_samples': 1, 'loopguard.envelope_pct': 50}, {'microskills': 5}, {'microskills': 1.5}, {'loopguard': None}]
for v in vals:
    with open(syn + '/config.json', 'w') as f:
        json.dump(v, f)
    n += 1; bad += cmp(f'cfg:{v}', base, syn)
os.remove(syn + '/config.json')
for e in [{'TE_MODE': 'headless'}, {'TE_DISABLE': '1'}, {'TE_LOOPGUARD': 'stop'}, {'TE_SPIRAL': 'off', 'TE_LOOPGUARD': 'nudge'}, {'TE_LOOPGUARD_THRESHOLDS': '{bad'}, {'TE_SPIRAL_THRESHOLDS': '{"min_turns":4}'}, {'TE_TOOLPRUNE_PROTECT': 'a,b'}, {'TE_REPOMAP_TOKENS': 'x'}]:
    n += 1; bad += cmp(f'env:{e}', base, syn, **e)
# State files.
key = json.loads(run([RS, '--cwd', base, 'status', '--json'], env(syn))[1])['state']; os.makedirs(key, exist_ok=True)


def wr(f, o):
    with open(os.path.join(key, f), 'w') as fh:
        fh.write(json.dumps(o))


wr('injection.json', {'rendered': '# t\n- a\n- b\nc\n', 'sessions': 4})
wr('untool.json', {'sessions': 3, 'prompts': 9, 'catalog_source': 'init', 'proposal': ['WebFetch', 'é']})
set_ = os.path.join(base, '.claude', 'settings.local.json')
wr('untool_applied.json', {'by_path': {set_: ['A', 'B']}}); n += 1; bad += cmp('applied', base, syn)
wr('untool_applied.json', {'tools': ['O'], 'path': set_}); n += 1; bad += cmp('applied-old', base, syn)
wr('untool_applied.json', {'by_path': {'': ['L']}}); n += 1; bad += cmp('applied-any', base, syn)
wr('envelope.json', {'resolved_turns': [3, 0, 8, 20, 41, 5, 6, 9, 11, 13, 2], 'resolved_snaps': [{}, 1]}); n += 1; bad += cmp('envelope', base, syn)
with open(os.path.join(key, 'ledger.jsonl'), 'w') as fh:
    fh.write('\n'.join(json.dumps(r) for r in [{'kind': 'session', 'effective_tokens': 1500.5, 'requests': 3, 'nudges': 1, 'aborts': 0, 'turns': 4, 'injected_chars': 9, 'overhead': {'spiral_judge_tokens': 120, 'x': 5}}, {'kind': 'session', 'effective_tokens': 2000000, 'turns': 1}, {'kind': 'overhead', 'overhead': {'optimizer_tokens': 33.5}}, {'kind': 'other', 'nudges': 5}]) + '\nnot json\n')
n += 1; bad += cmp('ledger', base, syn)
# A session pinned elsewhere, and a merged pin.
pins = os.path.join(syn, 'sessions', 'claude'); os.makedirs(pins)
with open(pins + '/s1.json', 'w') as fh:
    json.dump({'root': 'C:\\elsewhere', 'key': 'other-1', 'label': 'o/o', 'remote': '', 'how': 'path', 'at': 5}, fh)
os.makedirs(syn + '/repos/other-1'); n += 1; bad += cmp('pin', base, syn, CLAUDE_CODE_SESSION_ID='s1')
with open(pins + '/s2.json', 'w') as fh:
    json.dump({'root': 'C:\\gone', 'key': 'gone-1', 'label': 'g', 'remote': '', 'how': 'path', 'at': 5}, fh)
with open(syn + '/index.json', 'w') as fh:
    json.dump({'redirects': {'gone-1': {'to': 'other-1', 'at': 9}}}, fh)
n += 1; bad += cmp('pin-merged', base, syn, TE_SESSION_ID='s2')
print(f'{n} cases, {bad} differences'); print('workdir', work)
