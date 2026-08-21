# "Building a Prop AMM with Claude" — verbatim excerpts

Source: <https://www.benedict.dev/prop-amm>, published 2026-01-08, author Benedict Brady.
Byte-exact page snapshot: `blog-benedict-dev-prop-amm.html` in this directory (fetched
2026-08-21). The passages below are transcribed verbatim from that snapshot's
server-rendered content (a Next.js RSC payload embedded in the static HTML — confirmed
present in the served markup itself, not requiring client-side JS to produce), quoted
exactly rather than paraphrased, so the fidelity note in `README.md` can cite exact text
instead of a third-party summary.

## The toy model (single linear range per side)

> The simplest possible Prop AMM is one that provides liquidity linearly between two
> values on opposite sides of the order book.

> In our example, given a fair value for SOL of $200, a liquidity provider offers linear
> liquidity from 194-199 and 201-206. On the offer side of the book the trader is
> providing 10 SOL of liquidity and on the bid side they are providing 2000 USDC.

```python
class BookSide(BaseModel):
    quantity: float
    lower_price: float
    upper_price: float

    @property
    def liquidity_per_price_unit(self) -> float:
        return self.quantity / (self.upper_price - self.lower_price)

    def buy_exact_in(self, notional: float) -> float:
        quantity_bought = self.liquidity_per_price_unit * (math.sqrt(self.lower_price
            ** 2 + 2 * notional / self.liquidity_per_price_unit) - self.lower_price)
        self.lower_price += quantity_bought / self.liquidity_per_price_unit
        self.quantity -= quantity_bought
        return quantity_bought

    def buy_exact_out(self, quantity: float) -> float:
        notional_bought = quantity * self.lower_price + quantity ** 2 / (2 * self
            .liquidity_per_price_unit)
        self.lower_price += quantity / self.liquidity_per_price_unit
        self.quantity -= quantity
        return notional_bought
```

## The general design (what was actually shipped as `benedictbrady/prop-amm`)

> The toy model above uses a single linear range for each side of the book. A natural
> extension is to use multiple price points, creating a piecewise linear curve that can
> approximate more complex liquidity distributions.

> I worked with Opus to design a more general liquidity curve that has 7 points per side
> of the book. This system also improves on the previous design by using the quantity
> from a swap to replenish liquidity on the other side of the book. These small
> improvements make for a reasonably flexible Prop AMM that a sophisticated market maker
> could use to compete onchain.

> Next, I used Claude Code to mock this program, deploy it on mainnet, and build a simple
> GUI to test it with real money. […] The source code is available here [links to
> `benedictbrady/prop-amm`]. […] The program has 150 CU oracle updates, which is within
> range of state of the art.

This directly confirms the 7-point/6-segment, self-replenishing design copied into
`state.rs`/`math/piecewise.rs`/`instructions/swap.rs` in this directory is the actual
shipped mechanism, not a separate description.

## Oracle Staleness Backoff — the exploratory, unimplemented mock-up

> There are three axes on which to improve the Prop AMM architecture.
> - Off-chain fair value generation and quoting logic
> - Parameterization of the liquidity curve
> - Post-fill callback logic
>
> […] To test Claude's ability to come up with novel designs, I gave it some light
> guidance and then asked it to mock up some potential improvements. You can see three
> of them explained below. They are not terrible but they are not above the level of a
> competent market maker. The key to improving this process in the future is to add a
> verification loop where the model can validate its ideas.

> **Oracle Staleness Backoff**
>
> When the off-chain oracle stops updating, the onchain fair value becomes increasingly
> unreliable. Rather than quoting stale prices indefinitely, the system can progressively
> widen spreads as staleness increases, eventually pulling quotes entirely.
>
> This creates a natural defense against adverse selection during oracle outages or
> network congestion. The exponential backoff ensures that short delays have minimal
> impact on normal trading, while extended staleness triggers protective behavior.
>
> *[accompanying chart caption]* Spread widens exponentially with oracle staleness.
> After 30 seconds, spreads begin widening aggressively. After 45 seconds, quotes are
> pulled entirely.

This is one of **three** mocked-up "axes of improvement" (the others: inventory skew,
depth placement) that the post explicitly frames as exploratory and "not above the level
of a competent market maker" — i.e. the author's own assessment is that these were
*not* validated designs. No formula is given anywhere in the post — only the qualitative
chart (spread % vs. seconds since last oracle update, with a 30s "Widen" boundary and a
45s "Pull Quotes" boundary). This is the entire basis for `README.md`'s fidelity caveat
on this detail.
