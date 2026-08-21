# Prop AMM Design & Analysis


## Evolution of the Competition
While the [*Simple AMM competition*](/6i1LNsRGRJ27YqTHjfO04A) restricted strategies to fee adjustment within a constant-product ($xy=k$) framework, the Prop AMM sequel grants full control over the *entire swap function*.

| Feature | Simple AMM | Prop AMM |
| :--- | :--- | :--- |
| Control Parameter | Fee rate ($\gamma_{\pm}$) | Price Impact Function ($f_{\pm}$) |
| Curve Geometry | Fixed Constant-Product | Arbitrary/Custom |
| Volatility ($\sigma$) | $0.088\% - 0.101\%$ | $0.01\% - 0.70\%$ |
| Success Metric | Total Edge | Hedged PnL (HPnL) |

**The Goal**: Maximize the profit earned relative to the fair market price by optimizing the trade-off between capturing retail spreads and minimizing arbitrage leakage in a high-volatility environment.



## Scoring: Hedged PnL (HPnL)
Performance is calculated using *Hedged PnL*, which measures the final value of all net inventories priced at the terminal fair price $p_T$

$$
\textrm{HPnL} = \left(\sum_t \Delta x_t\right) \cdot p_T + \left(\sum_t \Delta y_t\right)
$$

* *Edge vs. HPnL*: While "Edge" effectively assumes hedging at every individual step, HPnL accounts for the cumulative inventory risk held until the end of the simulation cycle.
* *Competition*: Strategies are benchmarked against a Normalizer AMM (fixed 30 bps Uniswap V2) across 1,000 simulations of 10,000 steps each.



## The Prop AMM Trading Function
The Prop AMM is defined by its marginal price functions $p^+$ and $p^-$, which determine the price impact based on the current inventory and the size of the trade. Instead of a fixed curve, you define the output $\Delta y$ as the integral of your price function.


### General Integral Form
The total amount of token Y exchanged ($\Delta y$) for a given change in token X ($\Delta x$) is the integral of the marginal price function:

* *Trader Sells X (AMM Buys)*: $\Delta y = -\int^{\Delta x}_{0} p^+(x) dx$ for $\Delta x > 0$.
* *Trader Buys X (AMM Sells)*: $\Delta y = -\int^{\Delta x}_{0} p^-(x) dx$ for $\Delta x < 0$.


### Uniswap as a Special Case
Uniswap V2 serves as a benchmark for the Prop AMM by following the $x \cdot y = k$ constant-product model with a fixed fee tier $\gamma$.

#### Retail Sell (Trader Sells X / AMM Buys X)
The trading function is defined implicitly by $(x + \gamma \Delta x)(y + \Delta y) = k$. Solving for $\Delta y$:

$$
\Delta y = \frac{k}{x + \gamma \Delta x} - y
$$

The marginal buy price $p^+(\Delta x)$ is:

$$
p^+(\Delta x) = - \frac{d \Delta y}{d \Delta x} = \frac{k \gamma}{(x + \gamma \Delta x)^2}
$$

#### Retail Buy (Trader Buys X / AMM Sells X)
The trading function is defined by $(x + \Delta x)(y + \gamma \Delta y) = k$. Solving for $\Delta y$:

$$
\Delta y = \frac{1}{\gamma} \left( \frac{k}{x + \Delta x} - y \right)
$$

The marginal sell price $p^-(\Delta x)$ is:

$$
p^-(\Delta x) = - \frac{d \Delta y}{d \Delta x} = \frac{k}{\gamma(x + \Delta x)^2}
$$


### Linear Price Impact Model
In this model, price changes are linear relative to the order size:

\begin{align}
\Delta p^+ &= - k_{++} \Delta x^+ + k_{+-} \Delta x^- \\
\Delta p^- &= - k_{-+} \Delta x^+ + k_{--} \Delta x^-
\end{align}

Integrating these functions yields the quadratic trading function:

$$
\Delta y = 
\begin{cases}
{-} p^+ \Delta x^+ + \frac{1}{2} k_{++} (\Delta x^+)^2 & \text{if } \Delta x > 0 \text{ (AMM Buys X)} \\
p^- \Delta x^- - \frac{1}{2} k_{--} (\Delta x^-)^2 & \text{if } \Delta x < 0 \text{ (AMM Sells X)}
\end{cases}
$$



## The Simulation
The Prop AMM simulation environment is designed to test the robustness of your custom trading functions over 10,000 discrete steps. At each step:

1. Price moves — A fair price $p$ evolves via geometric Brownian motion
2. Arbitrageurs trade — They push each AMM's margin price toward $s$, extracting profit
3. Retail orders arrive — Random buy/sell orders get routed optimally across AMMs


### Price Evolution
The fair market price $s$ (the external "true" price) follows *Geometric Brownian Motion (GBM)* with zero drift. This ensures there is no directional bias, forcing the strategy to rely on volatility and volume estimation rather than trend-following.

$$
s(t+1) = s(t) \cdot \exp(- \frac{\sigma^2}2 + \sigma Z), \quad Z \sim N(0,1)
$$

* *Volatility ($\sigma$)*: Unlike the Simple AMM, the per-step volatility here is sampled from a significantly wider range of $U[0.01\%, 0.70\%]$ per simulation.
* *Impact*: High-volatility simulations will rapidly push $s$ away from your marginal prices, leading to significant arbitrage pressure if your price impact slopes ($k_{ii}$) are too shallow.


### Arbitrage Dynamics
Arbitrageurs act as rational agents that extract profit whenever your AMM’s marginal price is "stale" compared to the fair price $s$. They trade until your marginal price equals the market price ($p^+ = s$ or $p^- = s$).

In the Linear Price Impact model, the arbitrage update also accounts for cross-impact:

* *If $s < p^+$ (Overpriced X)*: Arbitrageurs sell X to the AMM until $p^+ = s$.
	* *Cross-Impact Update*: The opposite side of the AMM is adjusted based on the cross-impact coefficient: $$p^- = p^- - \frac{k_{-+}}{k_{++}} (p^+ - s)$$
* If $s > p^-$ (Underpriced X): Arbitrageurs buy X from the AMM until $p^- = s$.
	* *Cross-Impact Update*: $$p^+ = p^+ + \frac{k_{+-}}{k_{--}} (s - p^-)$$


### Trade Arrival (Retail Flow)
Retail orders represent uninformed liquidity that provides the primary source of "Edge" or profit for the AMM.

* *Poisson Process*: Orders arrive at a rate $\lambda \sim U[0.4, 1.2]$ per simulation.
* *Order Size*: Follows a LogNormal distribution with $\mu \sim U[12, 28]$ in $Y$ terms and $\sigma = 1.2$.
* *Directional Parity*: Each order has a 50% chance of being a buy or a sell.


### Order Routing Solver
Because you have full control over the price function, retail orders are routed optimally across AMMs to minimize total cost for the trader. This is achieved by equalizing the *post-trade marginal prices* of your AMM and the Normalizer.

For a total retail order of size $Y$, the solver must find a split $(\Delta y_1, \Delta y_2)$ such that:

1. Price Equivalence: $p^+_1(\Delta y_1) = p^+_2(\Delta y_2)$ (for buys) or $p^-_1(\Delta y_1) = p^-_2(\Delta y_2)$ (for sells).
2. Volume Constraint: $\Delta y_1 + \Delta y_2 = Y$.

#### The Numerical Challenge
Unlike the constant-product formula, custom $p(x)$ functions may not have a closed-form solution for the split. A numerical solver is used to find the equilibrium point $p^*_{final}$ that satisfies:

$$
\Delta y_1(p^*_{final}, p^*_{init,1}) + \Delta y_2(p^*_{final}, p^*_{init,2}) = Y
$$



## Reference
* https://www.ammchallenge.com/prop-amm
* https://github.com/benedictbrady/prop-amm-challenge