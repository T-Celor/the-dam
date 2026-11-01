"""The same questions to graphify query and dam graph query: does the answer name the thing that answers it?"""
import subprocess, sys, time

BIN, G, W = sys.argv[1], sys.argv[2], sys.argv[3]
QS = [
    ('TokenEconomizer', 'how does toolprune apply the deny list', ['denylist.py', 'apply']),
    ('TokenEconomizer', 'where is the failure loop guard threshold decided', ['loopguard']),
    ('TokenEconomizer', 'how are micro skill facts distilled from sessions', ['distiller']),
    ('TokenEconomizer', 'what writes the session ledger of effective tokens', ['accounting']),
    ('TokenEconomizer', 'who calls fold after a session ends', ['fold']),
    ('Optimism', 'how is the backtest simulated', ['BacktestSimulator']),
    ('Optimism', 'who computes the sharpe ratio', ['Sharpe']),
    ('Optimism', 'how are yahoo finance prices fetched', ['YahooFinance']),
    ('Optimism', 'how is the monte carlo simulation run', ['MonteCarlo']),
    ('Optimism', 'how does the quadrant matrix get calculated', ['calculateMatrix']),
    ('headroom', 'how does the log compressor keep error lines', ['log_compressor']),
    ('headroom', 'how does the search compressor limit matches per file', ['search_compressor']),
    ('headroom', 'where is the ccr store retrieve implemented', ['ccr']),
    ('headroom', 'how does content router pick a compressor', ['content_router']),
    ('headroom', 'how does smart crusher detect anomalies', ['smart_crusher']),
]


def ask(cmd, cwd):
    t = time.perf_counter()
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding='utf-8', errors='replace')
    return r.stdout + r.stderr, time.perf_counter() - t


tot = {'graphify': [0, 0, 0.0, 0], 'dam': [0, 0, 0.0, 0]}
for repo, q, want in QS:
    cwd = f'{W}/{repo}'
    row = [f'{repo[:6]:6} {q[:50]:50}']
    for name, cmd in (('graphify', [G, 'query', q]), ('dam', [BIN, 'query', q])):
        out, took = ask(cmd, cwd)
        lines = out.splitlines()
        where = next((i for i, l in enumerate(lines) if all(w.lower() in l.lower() for w in want)), None)
        hit = where is not None
        tot[name][0] += hit
        tot[name][1] += 1
        tot[name][2] += took
        tot[name][3] += len(out) // 4
        row.append(f'{name}: {"line " + str(where) if hit else "NOT FOUND":10} {len(out) // 4:5}t {took:5.2f}s')
    print(' | '.join(row))
for name, (hit, n, took, tokens) in tot.items():
    print(f'{name}: answered {hit}/{n}, mean {tokens // n} tokens, mean {took / n:.2f}s')
