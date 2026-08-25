# U256 reference vector provenance

`arith-vectors.json` holds 1 706 cases of `add` / `sub` / `mul` / `wrapping_mul`
/ `div` / `rem` / `isqrt` over 256-bit operands. Every expected value was
computed with **Python arbitrary-precision integers**, i.e. by an implementation
entirely independent of `src/u256.rs`. `null` marks a case where the operation
would overflow (add/mul), underflow (sub) or divide by zero.

Values are hex strings so the file stays compact and unambiguous.

## Coverage

* all 16×16 pairs of edge values: `0`, `1`, `2`, `2^64 ± 1`, `2^128 ± 1`,
  `2^192 ± 1`, `2^255`, `uint256 max`, `max - 1`, `1e18`, `1e18 - 1`, `1e36`,
  `1e36 - 1`
* 900 random pairs with bit widths drawn from
  `{1, 8, 32, 63, 64, 65, 96, 127, 128, 129, 160, 192, 200, 255, 256}` — this is
  what exercises the three division paths (single-limb, `u128` fast path,
  Knuth D)
* 150 `a == b` pairs
* 400 pairs deliberately shaped as "256-bit dividend ÷ >64-bit divisor", the
  Knuth-D path that the Flashbots `K / base` quote takes on every call

## Regenerate

```python
import random, json, math
random.seed(0xC0FFEE)
MAX = (1 << 256) - 1
def h(v): return "0x%x" % v
def rnd():
    bits = random.choice([1,8,32,63,64,65,96,127,128,129,160,192,200,255,256])
    return random.getrandbits(bits) & MAX
edges = [0,1,2,MAX,MAX-1,(1<<64)-1,1<<64,(1<<128)-1,1<<128,(1<<192)-1,1<<192,
         10**18,10**36,10**18-1,10**36-1,2**255]
pairs = [(a,b) for a in edges for b in edges]
for _ in range(900): pairs.append((rnd(), rnd()))
for _ in range(150):
    a = rnd(); pairs.append((a,a))
for _ in range(400):
    a = random.getrandbits(random.choice([200,240,256])) & MAX
    b = (random.getrandbits(random.choice([100,120,128,130,160])) or 1) & MAX
    pairs.append((a,b))
cases = []
for a,b in pairs:
    c = {"a": h(a), "b": h(b)}
    s = a+b;  c["add"] = h(s) if s <= MAX else None
    c["sub"] = h(a-b) if a >= b else None
    p = a*b;  c["mul"] = h(p) if p <= MAX else None
    c["wmul"] = h(p & MAX)
    c["div"] = h(a//b) if b else None
    c["rem"] = h(a%b) if b else None
    c["isqrt"] = h(math.isqrt(a))
    cases.append(c)
json.dump({"count": len(cases), "cases": cases}, open('arith-vectors.json','w'))
```

## Note on `isqrt`

`tests/u256_reference.rs` compares `dodo_math::sqrt` (the ported Babylonian loop)
against `math.isqrt`. They agree for every input **except `x = 2`**, where the
upstream loop starts at `y = x` with `z = x/2 + 1 = 2`, never iterates, and
returns `2`. That is upstream behaviour at the pinned DODO commit and is
preserved deliberately; the test pins the exception explicitly rather than
papering over it.
