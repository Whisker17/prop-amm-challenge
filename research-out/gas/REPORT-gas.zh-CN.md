# Gas benchmark 结果（真实 EVM）

本文件由 `gas-harness/script/render-gas-report.py` 从 `gas-snapshot.json` 生成，不要手工编辑。全部数字由 Foundry 在真实 EVM 上用 `vm.lastCallGas()` 测得，没有任何一项来自 Rust 执行时间估算。

## 运行信息

| 项 | 值 |
| --- | --- |
| benchmark commit | `26fe1f1b651c3b93d91cce954919f1eb0016efcc` |
| 运行时工作区状态 | 干净 |
| forge | `1.4.1-v1.4.1` |
| 执行用 EVM 规格 | `prague` |
| TestToken runtime code hash | `0x3bf3d6d196134e6d841d22ae8ddb033181df908a63f8ea904b5bf8da9822d39d` |
| 第三方源码 | 不入库。`script/fetch-vendor.sh` 按 commit 拉取到被 git ignore 的 `lib/`，逐文件校验 sha256 后才编译 |
| pin 清单 | `gas-harness/vendor-pins.sha256` |
| 测量点数 | 240 |

## 三个数怎么读

每一行同时给出 `rawGas`、`nullTwinGas`、`adjustedGas = rawGas − nullTwinGas`。

- **`rawGas`** 是系统成本：含该算法自己的调用框架、selector 表形状、存储布局。
- **`nullTwinGas`** 是同 pragma、同签名、同 selector 集合、同存储布局的空壳成本（C 层例外，见该节）。
- **`adjustedGas`** 只是**诊断量**，用来把编译器 dispatch 税剥出去。它不是唯一结论，也不能替代 `rawGas`。

排名必须分两份给：raw system gas 一份，adjusted curve gas 一份。

## A 层：统一口径下的整笔报价

输入全部走 calldata，函数为 `view`（STATICCALL），每个算法通过形状完全相同的 adapter 调用，不碰存储。这是**可以**跨算法比较曲线数学的一层。

| 算法 | 层 | n | rawGas 区间 | rawGas 中位 | nullTwinGas | adjustedGas 中位 |
| --- | --- | --- | --- | --- | --- | --- |
| Uniswap V2（canonical 0.30%） | A | 20 | 2130 – 2137 | 2133 | 1402 | 731 |
| Uniswap V2（benchmark-only 零费） | A | 20 | 2130 – 2137 | 2133 | 1402 | 731 |
| Flashbots ExamplePropAmm | A | 20 | 2585 – 2592 | 2588 | 1651 | 937 |
| Uniswap V3（零费） | A2 | 20 | 10250 – 79078 | 12967 | 1402 | 11565 |
| DODO PMM | A | 20 | 22282 – 43003 | 24426 | 1651 | 22775 |

区间跨越 4 个状态（balanced / off-target）× 2 个方向 × 5 个 trade size。V3 的区间宽是因为 tick 穿越数不同（`tickCrossings` 列逐行记录）。

**V3 的 A 层拆成两个指标，二者不可互换：**

- `A1 primitiveStep`：单次 `SwapMath.computeSwapStep`，rawGas 3863 – 5198。它与**任何**整笔报价都不可比，**包括与自己的 A2 不可比**——上表里没有它。
- `A2 fullQuote`：从输入到最终输出的完整报价循环，含 tick 遍历。只有 A2 能与上表其它行放在一起。

## B 层：状态更新（两套 oracle 口径）

- **algorithm-only**：oracle 价格通过完全相同的 adapter calldata 槽送入，排除 registry / calldata 架构差异。**这是可跨算法比较的数字。**
- **reference-system**：各算法的真实路径（DODO 走 calldata、Flashbots 走 `PrioUpdateRegistry`、V3 写 observation、V2 写 price cumulative）。这是**系统成本**，**不得**用它论证某条曲线的数学更贵——它们是不同产品，不是同一功能的不同实现。

cold 用 `vm.coolSlot` 作用在**拥有存储的那个合约**上（作用在 wrapper 上无效）。

| 算法 | algorithm-only warm | algorithm-only cold | reference-system warm | reference-system cold |
| --- | --- | --- | --- | --- |
| Uniswap V2（canonical 0.30%） | 2716 – 2717 | 6716 – 6717 | 4374 – 4375 | 14374 – 14375 |
| Uniswap V2（benchmark-only 零费） | 2716 – 2717 | 6716 – 6717 | 4374 – 4375 | 14374 – 14375 |
| Flashbots ExamplePropAmm | 3346 – 3364 | 11346 – 11364 | 8682 – 8700 | 20782 – 20800 |
| Uniswap V3（零费） | 28238 – 28573 | 38238 – 38573 | 29701 – 30036 | 39701 – 40036 |
| DODO PMM | 23140 – 46115 | 35140 – 58115 | 23802 – 46777 | 33802 – 56777 |

左两列可比，右两列**不可**跨算法比。V3 不消费外部 oracle，algorithm-only 口径下 calldata 槽被接受并忽略。

**SSTORE 定标行**（`seedState-*`）：分别测 0→非 0 首写、非 0 覆写、同一 tx 内归零、以及 `vm.store` 预置后的归零。`rawGas` 是**毛计量值**，`gasRefunded` 是 EIP-3529 退款计数器——退款在交易结束时才结算且上限为 `gasUsed/5`，所以两列必须分开看，不能预先相减。

## C 层：完整 swap

含池架构、代币转账与存储差异，**不能把差异全部归因于曲线数学**。所有算法共用同一个最小 `TestToken`（runtime code hash 见上表）。C 层的 `nullTwinGas` 不是同 pragma 空壳，而是 **token-only 对照**：一次转出、一次外部调用、一次转回，不含任何曲线。raw 与对照值并列给出，不只给扣减值。

| 算法 | n | rawGas 区间 | rawGas 中位 | 减去 token-only 对照 |
| --- | --- | --- | --- | --- |
| token-only 对照（无曲线） | 5 | 35384 | — | 基线本身 |
| Uniswap V2（benchmark-only 零费） | 10 | 58186 – 58197 | 58191 | 22807 |
| Uniswap V2（canonical 0.30%） | 10 | 58612 – 58623 | 58617 | 23233 |
| DODO PMM | 10 | 96510 – 98254 | 97506 | 62122 |
| Flashbots ExamplePropAmm | 10 | 103682 – 103700 | 103691 | 68307 |
| Uniswap V3（零费） | 10 | 65986 – 275136 | 120787 | 85403 |

Uniswap V2 出现两行且**不可混用**：`canonical-30bps` 是未改动的上游生产实现；`modified-zero-fee` 是 benchmark-only 的零费改造，补丁见 `gas-harness/patches/`，改后 bytecode hash 见 `bytecode-hashes.csv`。零费主对比用后者，前者单独展示。

### DODO 与 Flashbots 在 C 层不可直接排名

> C 层的 `DodoPool` 是**本 harness 自己写的最小池**，只包住 pinned `PMMPricing` 库；`ExamplePropAmm` 是**完整的生产合约**，带 pair 注册表、跨合约读 `PrioUpdateRegistry`、`Ownable`/`ReentrancyGuard`/`SafeERC20`。二者是不同完成度的产物，不是同一件事的两种实现，因此 C 层这两行**不构成排名**。
>
> 这一点在数据上是可见的：C 层 DODO 中位 97506、Flashbots 中位 103691，**顺序与 A/B 层相反**。这个反转衡量的是包装层的完成度差异，**不推翻**下节在统一口径下的结论，也**不能**被反过来当作 DODO 曲线更便宜的证据。
>
> 若要做最终产品级对比，需要接入真实的 `MantlePropAmmPool`；本 harness 目前没有它，所以本报告不给出产品级 gas 排名。

### 费率敏感性（单独一节，不并入零费主表）

| 算法 | n | rawGas 区间 |
| --- | --- | --- |
| DODO PMM（lpFee 0.03%） | 2 | 96835 – 98178 |
| Uniswap V3（canonical 0.30%） | 2 | 179914 – 210136 |

## 可以下的结论

**在统一的 quote 与 state-update 口径下，Flashbots 明显比 DODO 便宜。**

| 口径 | Flashbots | DODO | DODO / Flashbots |
| --- | --- | --- | --- |
| A 层整笔报价 rawGas 中位 | 2588 | 24426 | 9.4× |
| B 层 algorithm-only 状态更新 rawGas 中位 | 3355 | 44321 | 13.2× |

两个口径方向一致，且都排除了架构与 oracle 路径差异。

**只有上面这一条跨算法结论成立。** 其余排名都不成立：

- C 层各实现完成度不同（见上节），其顺序反转不构成证据；
- reference-system 口径衡量的是产品而非曲线；
- `adjustedGas` 只是诊断量；
- V3 的 A1 与任何整笔报价不可比。

本报告不回答“哪条曲线在产品上更省 gas”——那需要真实的 `MantlePropAmmPool`。

## 不可比较项与混淆因素

- 编译器 dispatch 税按签名而异，符号还会翻转（`noop()` 在 0.5/0.6/0.7 是 166、在 0.8.28 是 141；`ident(uint256)` 反过来是 215 vs 233）。null-twin 扣减只有在 twin 的 selector 集合与签名完全一致时才成立，这一点由 `test/NullTwin.t.sol` 在写任何数字之前断言。
- selector 表形状不可消除，只能披露。
- 池/代理架构与调用深度不同。
- oracle 读取路径是不同产品。
- 每个算法按其上游 canonical 配置编译（pragma 硬钉，无法统一），逐行记录在 `compilerConfig` 列，由 `foundry.toml` 的 `compilation_restrictions` 落实。
- V3 的 gas 强烈依赖 tick 穿越数，逐行见 `tickCrossings`；跨算法比较时必须连它一起读。

## 产物

```
research-out/gas/gas-snapshot.csv       每个测量点一行
research-out/gas/gas-snapshot.json      同样的行 + 运行级 metadata（含 benchmarkCommit）
research-out/gas/touched-slots.csv      B 层 vm.record()/vm.accesses() 结果
research-out/gas/bytecode-hashes.csv    C 层每个合约的部署 runtime code hash
research-out/gas/REPORT-gas.zh-CN.md    本文件
research-out/gas/MANIFEST.sha256        以上全部文件的 sha256
```

复现：`cd gas-harness && ./script/gas-snapshot.sh`。该脚本会先按 pin 重新拉取并校验第三方源码，任何 sha256 不符都会在编译之前中止。
