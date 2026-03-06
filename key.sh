#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
INPUT_FILE="${1:-$ROOT_DIR/key.txt}"

if [ ! -f "$INPUT_FILE" ]; then
  echo "Input file not found: $INPUT_FILE"
  echo "Usage: ./key.sh [path/to/key.txt]"
  exit 1
fi

python3 - "$INPUT_FILE" <<'PY'
import math
import pathlib
import re
import statistics
import sys
from collections import Counter


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def parse_bits(raw_bytes: bytes) -> list[int]:
    text = raw_bytes.decode("utf-8", errors="ignore").strip()
    text_no_ws = re.sub(r"\s+", "", text)

    # Accept plain bitstrings: 010101...
    if text_no_ws and re.fullmatch(r"[01]+", text_no_ws):
        return [1 if c == "1" else 0 for c in text_no_ws]

    # Accept hex bytes: "de ad be ef" or "deadbeef"
    if text_no_ws and re.fullmatch(r"[0-9a-fA-F]+", text_no_ws) and len(text_no_ws) % 2 == 0:
        data = bytes.fromhex(text_no_ws)
    else:
        data = raw_bytes

    bits: list[int] = []
    for b in data:
        for shift in range(7, -1, -1):
            bits.append((b >> shift) & 1)
    return bits


def shannon_entropy_binary(bits: list[int]) -> float:
    n = len(bits)
    if n == 0:
        return 0.0
    ones = sum(bits)
    zeros = n - ones
    h = 0.0
    for c in (zeros, ones):
        if c == 0:
            continue
        p = c / n
        h -= p * math.log2(p)
    return h


def min_entropy_binary(bits: list[int]) -> float:
    n = len(bits)
    if n == 0:
        return 0.0
    p0 = (n - sum(bits)) / n
    p1 = 1.0 - p0
    return -math.log2(max(p0, p1))


def conditional_entropy(bits: list[int]) -> tuple[float, float]:
    # Returns:
    # 1) Shannon conditional entropy H(X_i | X_{i-1}) in bits/bit
    # 2) Min-entropy-style conditional bound: -log2(max P(next | prev))
    if len(bits) < 2:
        return 0.0, 0.0

    trans = [[0, 0], [0, 0]]
    prev_counts = [0, 0]
    for a, b in zip(bits[:-1], bits[1:]):
        trans[a][b] += 1
        prev_counts[a] += 1

    h_cond = 0.0
    max_cond_p = 0.0
    total_prev = sum(prev_counts)
    for prev in (0, 1):
        if prev_counts[prev] == 0:
            continue
        p_prev = prev_counts[prev] / total_prev
        row_h = 0.0
        for nxt in (0, 1):
            c = trans[prev][nxt]
            if c == 0:
                continue
            p = c / prev_counts[prev]
            row_h -= p * math.log2(p)
            max_cond_p = max(max_cond_p, p)
        h_cond += p_prev * row_h

    hmin_cond = 0.0 if max_cond_p <= 0 else -math.log2(max_cond_p)
    return h_cond, hmin_cond


def serial_correlation(bits: list[int]) -> float:
    if len(bits) < 2:
        return 0.0
    x = bits[:-1]
    y = bits[1:]
    mx = statistics.fmean(x)
    my = statistics.fmean(y)
    sx2 = sum((v - mx) ** 2 for v in x)
    sy2 = sum((v - my) ** 2 for v in y)
    if sx2 <= 0 or sy2 <= 0:
        return 0.0
    cov = sum((a - mx) * (b - my) for a, b in zip(x, y))
    return cov / math.sqrt(sx2 * sy2)


def nist_mcv_lower_bound(bits: list[int]) -> float:
    # SP 800-90B most-common-value style bound for binary symbols.
    n = len(bits)
    if n == 0:
        return 0.0
    c = Counter(bits)
    p_hat = max(c.values()) / n
    # 99% one-sided normal upper confidence approximation for p_max.
    z = 2.576
    sigma = math.sqrt(max(0.0, p_hat * (1.0 - p_hat) / max(1, n - 1)))
    p_u = clamp(p_hat + z * sigma, 0.0, 1.0)
    if p_u <= 0:
        return 0.0
    return -math.log2(p_u)


def nist_markov_lower_bound(bits: list[int]) -> float:
    # SP 800-90B Markov style conservative per-bit bound (binary).
    if len(bits) < 2:
        return 0.0
    trans = [[0, 0], [0, 0]]
    row_totals = [0, 0]
    for a, b in zip(bits[:-1], bits[1:]):
        trans[a][b] += 1
        row_totals[a] += 1

    # Conservative: highest conditional transition probability.
    max_p = 0.0
    for r in (0, 1):
        if row_totals[r] == 0:
            continue
        for c in (0, 1):
            max_p = max(max_p, trans[r][c] / row_totals[r])
    if max_p <= 0:
        return 0.0
    return -math.log2(max_p)


def nist_collision_style_lower_bound(bits: list[int]) -> float:
    # Collision-style bound using distances to same-bit recurrence.
    # Conservative mapping: p ~= 1 / mean_distance.
    if len(bits) < 3:
        return 0.0
    distances: list[int] = []
    run_len = 1
    for i in range(1, len(bits)):
        run_len += 1
        if bits[i] == bits[i - 1]:
            distances.append(run_len)
            run_len = 1
    if not distances:
        return 0.0
    mean_d = statistics.fmean(distances)
    if mean_d <= 0:
        return 0.0
    p = clamp(1.0 / mean_d, 0.0, 1.0)
    if p <= 0:
        return 0.0
    return clamp(-math.log2(p), 0.0, 1.0)


path = pathlib.Path(sys.argv[1])
raw = path.read_bytes()
bits = parse_bits(raw)
n = len(bits)

if n < 16:
    print(f"Not enough data: {n} bits. Need at least 16 bits.")
    sys.exit(2)

h_shannon = shannon_entropy_binary(bits)
h_min = min_entropy_binary(bits)
h_cond, h_cond_min = conditional_entropy(bits)
rho = serial_correlation(bits)

h_mcv_90b = nist_mcv_lower_bound(bits)
h_markov_90b = nist_markov_lower_bound(bits)
h_collision_90b = nist_collision_style_lower_bound(bits)

# Lower bound: strict conservative choice among min-entropy style estimators.
lower_bound = max(0.0, min(h_min, h_cond_min, h_mcv_90b, h_markov_90b, h_collision_90b))

# Upper bound: constrained by Shannon entropy, then adjusted by observable dependence.
dependence_penalty = 1.0 - abs(rho)
upper_bound = min(1.0, h_shannon, h_cond / max(1e-12, dependence_penalty))
upper_bound = clamp(upper_bound, lower_bound, 1.0)

print(f"Input: {path}")
print(f"Bits analyzed: {n}")
print("")
print("Core estimators (bits per bit)")
print(f"  Shannon entropy              : {h_shannon:.6f}")
print(f"  Min-entropy                  : {h_min:.6f}")
print(f"  Conditional entropy H(X|X-1) : {h_cond:.6f}")
print(f"  Conditional min-entropy      : {h_cond_min:.6f}")
print(f"  Serial correlation (lag-1)   : {rho:.6f}")
print("")
print("NIST SP 800-90B style estimators (conservative)")
print(f"  MCV estimate lower bound     : {h_mcv_90b:.6f}")
print(f"  Markov estimate lower bound  : {h_markov_90b:.6f}")
print(f"  Collision-style lower bound  : {h_collision_90b:.6f}")
print("")
print("Entropy-per-bit bounds for key material")
print(f"  LOWER bound                  : {lower_bound:.6f}")
print(f"  UPPER bound                  : {upper_bound:.6f}")
PY
