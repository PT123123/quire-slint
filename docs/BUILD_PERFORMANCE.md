# 构建加速方案

「`just deploy-workshop-release` 要 2 分半」这件事已经被问过两次，两次的答案都是
「这是一个 crate 的 codegen，不是冷缓存」。本文把这句话拆开：**哪些部分已经
被证明、哪些杠杆还开着、哪些被提过但其实不成立**，并给出分级的执行顺序。

（2026-10-20 的名字更正：`just deploy-workshop` 现在指 **dev 档案**那一条，动一个 `.rs` 是 11–15 秒；
本文算的是 `deploy-workshop-release` 的 `--release` 代价。dev 那一棵树在 2026-10-20 起把**依赖**编成
优化的，改 profile 当轮要先付一次 1040 秒的全树重编，见 §12。）

状态：**评估与方案，未接入**（截至 `quire` 0.1.24）。本文没有改任何构建配置，
下面任何一条被采纳都要单独走一次「改代码 + 记 ADR」。文末的
**尚未验证**清单是这份文档的诚实边界。

原始数字全部来自 `docs/PERFORMANCE.md` 的 *Build cost (2026-09-25)* 一节，
本文不重新测量它们，只做**一次独立的复核**（见 §2）与**一次纠正**（见 §3）。

## 1. 结论先说

> 不是「没有增量编译」，而是**增量的粒度只到 crate，而 crate 只有一个**。
> 依赖层增量是健康的；本地的 `quire` crate 一改就全编，而它恰好是全项目里
> 唯一一个 `codegen-units = 1` + thin LTO 的单元。

拆 crate 是**最后手段**，不是第一步 —— 这一条和两轮实测一致。

**但外部给出的方案把「修 `build.rs` 的 include graph」排在了 L1 第一位，而这一条
在本仓库里已经不需要做了**（§3）。这是本文与那份方案最大的分歧，也是它存在的
主要理由。

## 2. 证据：什么快、什么慢

基准机：Intel Core Ultra 9 185H / 22 逻辑核 / 32 GiB / Windows 11。
`docs/PERFORMANCE.md` *Build cost* 一节的原始测量：

| 场景 | 命令 | 墙钟 | 是什么 |
|---|---|---:|---|
| 无改动（热） | `cargo build --release` | **4.4 s** | 依赖图与 crate 全判 Fresh —— 增量机制在线 |
| 改一个 `.rs` | `cargo build --release` | **2 m 30 s** | 唯一的本地 crate 重做 codegen + thin LTO |
| 同上，`codegen-units = 16` | `cargo build --release` | **3 m 07 s** | 反而更慢：ThinLTO 合并开销 > 并行收益 |
| 切换 `codegen-units` | `cargo build --release` | **7 m 59 s** | ≈240 个依赖 crate 因 profile 变更重建 |
| 改一个 `.rs`（dev） | `cargo build` | **1 m 08 s** | dev 增量在工作，只是单 crate 内生成代码量大 |

**本次复核（2026-10-05，`72a69f9`）**，同机同 profile，补 PERFORMANCE.md
没有的行，并把那 2 m 30 s **拆开**—— PERFORMANCE.md 自己写着「codegen 与 link 的
时间拆分从未被测过」，所以先测它，再谈优化：

| 场景 | 命令 | 墙钟 | 是什么 |
|---|---|---:|---|
| 改一个 `.rs`（dev，全 target） | `cargo check --workspace --all-targets` | **68.6 s** | 与 dev 的 1 m 08 s 同量级 |
| 改一个 `.rs`（release，全量） | `cargo build --release` | **209 s** | 本次实测，比 9-25 的 150 s 高（见下） |
| 改一个 `.rs` → 只重链接 | 删掉 exe 后 `cargo build --release --bin quire` | **11.2 s** | **纯 link，零 codegen** |
| 无改动（热） | `cargo build --release --bin quire` | **8.5 s** | 全 Fresh |
| `cargo tree` 解析 | — | **< 1 s** | 依赖图查询本身不是瓶颈 |

**209 s 里 link 只占 11.2 s，剩下 ≈198 s 全是一个 crate 的 codegen。**
这就是 PERFORMANCE.md 悬了许久的那一栏，现在有数字了：换链接器（`rust-lld` /
`lld-link`，实测存在于本 toolchain 的 `lib/rustlib/x86_64-pc-windows-msvc/bin/`）
**最多只能省那 11 秒**，而启用它要改 `linker-flavor`，那会作废全图指纹 →
≈240 个依赖 crate 重建（就是上面那 7 m 59 s）。**结论：不换，收益为零、代价为真。**

**codegen 为什么这么贵，量化如下**（同一 batch 实测）：

| 产物 | 大小 |
|---|---:|
| `target/release/deps/libquire-*.rlib` | **113.4 MB** |
| build script 生成的 `out/AppWindow.rs` | **24.2 MB / 225 648 行** |

`slint::include_modules!()` 把整个 UI 树展开成**一个** crate 里的 22 万行 Rust，
而 `codegen-units = 1` 规定这 22 万行必须在**一个** codegen unit 里过一遍再由
thin LTO 合并。**这是单 crate + AOT Slint 的结构性代价，任何 profile 旋钮都动不了它**
—— `cgu16` 更快只在全量构建（并行摊到多个依赖 crate）时成立，而这里的场景是
「重编一个 crate」，实测反而慢 24%。

> 209 s 与 9-25 那次的 150 s 的差：本仓库此后新增了大量 `.slint` 与 Rust 代码，
> 单 crate 从「一个大 crate」变成了「一个 113 MB rlib 的 crate」。
> **这不是回归，是同一个结构的量变**——两个数字都成立，引用时必须带日期与 commit。

与 dev 的 68.6 s 一并看：dev 的 68 秒**也不是缓存失效**——同一次运行里 Slint 前端
**没有重跑**（§3），那 68 秒是 rustc 在 `debug = 1` 下过同一批 22 万行。

### 结构事实（决定了上面所有数字）

- 根目录只有一个 `Cargo.toml`，`members = ["."]`。**不是多 crate workspace**，
  `--workspace` 是装饰（`justfile:19-23` 已经写明）。
- `src/lib.rs` 聚合 `app` / `platform`，并调用 `slint::include_modules!()` 把整个
  UI 编译进本 crate；领域核心在外部 rev-pinned 的 `quire-core`（`Cargo.toml:51`）。
- `[profile.release]` = `codegen-units = 1` + `lto = "thin"` + `strip`。
  这个组合已被 A3 审计（ADR-0024）**同时**认定为产物体积与重复构建速度的最优点：
  fat LTO 慢 2.5×，`cgu16` 在重编单 crate 时慢 24%。**不要为了增量去动它。**
- `just sweep` 已是正确的软清理：只删 `incremental/` + `build/`，保留 `deps/`
  与 `.fingerprint/`。`cargo clean` 才是敌人（ADR-0134）。

## 3. 纠正：`build.rs` 的 include graph 已经被覆盖了

外部方案把「收窄 `build.rs` 的 `rerun-if-changed`」列为**最高性价比、最低风险的
第一项改动**，理由是「`build.rs` 只声明了 `QUIRE_PROBE` 与 `install/quire.ico`，
未为 `ui/**/*.slint` 写声明，于是 Rust 改动也顺带重跑 Slint 前端」。

**前半句观察对了，后半句的结论错了。** `build.rs` 自己确实只声明了两条，但它
调用的 `slint_build::compile()` 会替它把整张 include graph 声明出来。

源码依据 —— `slint-build 1.18.0`（本仓库 `Cargo.lock:4558-4559` 锁定的版本）
`lib.rs:540-545`：

```rust
let paths_dependencies =
    compile_with_output_path(path, absolute_rust_output_file_path.clone(), config)?;
for path_dependency in paths_dependencies {
    println!("cargo:rerun-if-changed={}", path_dependency.display());
}
```

而 `paths_dependencies` 就是这张图（`lib.rs:610-634`）：`diag.all_loaded_files`
里的每个绝对路径 + 入口文件本身 + 每一个 embedded file resource。

**运行时证据** —— `target/debug/build/quire-*/output` 里实际落盘的内容，
一条 `QUIRE_PROBE` 之后是 **41 条 `.slint` 的 `rerun-if-changed`**，覆盖
`ui/Colors.slint`、`ui/Icons.slint`、`ui/Theme.slint`、`ui/TrayIcon.slint`、
`ui/Types.slint`、`ui/Typography.slint`、`ui/AppWindow.slint` 和
`ui/components/` 下全部 34 个组件，外加 `install/quire.png` 与 `install/quire.ico`。

**决定性实测** —— 改 `src/main.rs` 的 mtime（这正是源码编辑对 cargo fingerprint
做的事），然后跑 `cargo check --workspace --all-targets`：

```
build script dir      : quire-d614f274ece0d7c4
AppWindow.rs BEFORE   : 18:59:32
cargo check exit=0 in : 68.635 s
AppWindow.rs AFTER    : 18:59:32
VERDICT: Slint frontend did NOT re-run
```

68 秒花在了 rustc 上，**不是**花在 Slint 前端上。报告描述的那个症状
（「Rust 改动也顺带重跑 Slint 前端工作」）在这个仓库里**不存在**。

### 那份方案的 `build.rs` 补丁为什么不该做

三条独立理由，任何一条都足够：

1. **它要解决的问题不存在**（上面的实测）。加 `walkdir::WalkDir` 扫描 `ui/`
   只会把已经精确的 include graph 换成一个粗粒度的目录遍历。
2. **它编译不过**。`walkdir` 不在 `[build-dependencies]` 里（`Cargo.toml:87-89`
   只有 `embed-resource` 和 `slint-build`）。要么新增一个依赖，要么照抄一个
   引用了不存在 crate 的片段。
3. **它会往回退一步**。cargo 的规则是：一旦 build script 发出**任何**一条
   `rerun-if-changed`，就从「默认重跑」切到「只跟这些文件」。现在 shell 已经
   在后一种模式且清单是精确的；再补一批 `Cargo.toml` / `Cargo.lock` /
   `walkdir(ui/)` 只会**放宽**它，而放宽的方向正是 ADR-0134 那 63 GB 的成因。

同一条推理也**否掉了**方案里的 `println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION")`：
`[package].version` 本来就在 cargo 的 fingerprint 里，且 `build.rs` 读的
`CARGO_PKG_VERSION` 只在 build script **已经重跑**时才有意义 —— 而重跑与否
由上面那张表决定。声明它不会让版本 bump 变得更快，只会让版本 bump 之外的情况
多一次重跑。

## 4. 仍然成立的根因

排除掉 §3 之后，按「该动手的优先级」排：

**R1 · 单 crate × `codegen-units = 1`（结构性上限，就是那 2 分 30 秒）**
编辑任一 `.rs`，唯一 CGU 必须重生成。`incremental = true` 在 `cgu = 1` 下无意义
—— 增量复用的是 codegen unit，cgu=1 时整个 crate 就是一个 unit。依赖层完全正常
（4.4 s 那行就是证据），所以这不是「增量没开」，是**增量没有可以复用的粒度**。

**R2 · 版本号 bump 必然重编整个 crate（每次发版白付 2 分 30 秒）**
`build.rs` 把 `CARGO_PKG_VERSION` 打进 `OUT_DIR/quire.rc`，`justfile:57-58`
记录的顺序因此是刻意的：bump 必须先于 build。代价是每次
`deploy-workshop-release` / `release-publish` 都从 Fresh 掉回全编。**这是设计使然，
不是缺陷** —— exe 的版本资源块必须和 `Cargo.toml` 一致。

**这一条要与 ADR-0149 合读才完整**：那份 ADR 用 fingerprint 日志证明 bump **只**
让 `quire` 一个 crate 变脏，240+ 依赖全 Fresh，`Cargo.lock` 只差一行 ——
所以「bump 会重解析依赖 / 作废整图」在这里**不是**发生的事。两半拼起来才是
真正的形状：**代价不是「切换」，而是那唯一一个 crate 里有 22 万行生成代码**
（§2 实测的 198 s）。ADR-0149 也顺手否掉了两个想绕开它的办法（把版本挪进
`[package.metadata.app]`、用 `--cfg` / `rustflags` 注入）。

> 方案里「给 `.rc` 写入加内容稳定保护」在这里**帮不上忙**：版本 bump 时 `.rc`
> 的内容是**真的**变了，不重写反而是错的。它只在「build.rs 因别的原因重跑、
> 但版本没变」时省下一次 `rc.exe` 调用，收益接近零。

**R3 · 多 `--target-dir` 隔离了 feature（设计正确，但缓存策略必须尊重）**
`target-shot` / `target-skia` / `target-wgpu` 不能共享增量目录，因为 `software`
与默认 `femtovg` 是不同的 feature set（ADR-0134）。这是**对的**，不是缺陷；
磁盘预算要按这个边界算。

**R4 · 外部 Git 核心依赖**
`.cargo/config.toml` 的 `git-fetch-with-cli = true` 解决匿名 fetch
（libgit2 会拿凭证去问一个从未请求过认证的 URL，GitHub 回 401）。若将来引入
CI 而不缓存 `~/.cargo/git`，就会重复下载。

**R5 · 没有可审计的 CI 配置**
归档里**没有** `.github/workflows`，README 与 `justfile:16` 称 `just check` 是
「local CI replacement」，而 `just check` 是 check + test + release 三条串行命令。
所以**不能说「CI 缓存坏了」** —— 这里根本没有 CI。只能说：若重新引入，
必须按 profile/feature 分桶。

## 5. 分级方案

### L1 · 轻量（对症，但比方案说的少两项）

| | 动作 | 预期 | 何时 |
|---|---|---|---|
| **L1-A** | `just sweep` 作为默认清理，**禁常规 `cargo clean`** | 避免下次完整 codegen | ✅ 已具备，防误用即可 |
| **L1-B** | 加 `rust-toolchain.toml`（channel/components/target），提交 `Cargo.lock` | 挡掉 toolchain 漂移导致的整图重建 | 随时 |
| **L1-C** | 只在 `cargo check`/`test` 上验证 `CARGO_BUILD_JOBS = 逻辑核 − 2` | check/test 并行提速 | ≥8 核 |

**明确不做**（与原方案相反，理由在 §3 / §4-R2）：不收窄 `build.rs`（已经精确）、
不加 `walkdir` 依赖、不加「`.rc` 内容稳定写入」、不把 release 改成 `cgu = 16`
（实测慢 24%）、不为了增量动 `lto`。

**也不换链接器**（2026-10-05 实测后加入）：`rust-lld` / `lld-link` 确实在本
toolchain 里，但 §2 已经测出**整个 link 只占 11.2 s / 209 s**，而换
`linker-flavor` 要作废全图指纹（≈240 crate / 7 m 59 s）。**收益上限 11 秒，
代价是一次全量重建 —— 不做。**

**已被实测否决的两条**（写在这里以免重走）：

| 提议 | 实测 | 结论 |
|---|---|---|
| 把 `just check` 第三行从 `cargo build --release` 换成 release-opt-in 的 dev profile（`--profile dev` + `--config profile.dev.lto=…`） | **重建 325 个 crate** | ❌ 换 profile 作废全图指纹，与 `cgu` 那 7 m 59 s 同一个陷阱。它会让 `just check` **更慢**。且 `--profile` 与 `--release` 互斥，没有「便宜的 release 检查」这种 flag |
| 给 `CARGO_BUILD_JOBS = 逻辑核 − 2` | `cgu = 1` 下只有一个 unit 可并行 | ❌ 对单 crate 重编无收益，只作用于 `cargo check` 的多 crate 阶段，收益有限 |

`CARGO_BUILD_JOBS` 在 `cgu = 1` 下**不会**让单 crate 重编变快 —— 只有一个 unit
可并行。它只作用于 `cargo check` 的多 crate 阶段，收益有限，别当提速手段宣传。

### L2 · 中等（只在重新引入 CI 时）

- **三门分离**：`fast`（fmt + clippy + check）/ `test` / `release`。前两门共享
  缓存，`release` 另开 namespace —— 否则 profile 切换会让 `target/` 堆旧 unit
  （§2 那 7 m 59 s 的教训）。
- **两级 cache key**，不要只按 `Cargo.lock`：

  ```
  key: v1-${{ runner.os }}-${{ runner.arch }}-${{ env.RUSTC_VERSION }}
       -profile-${{ matrix.profile }}-features-${{ matrix.features }}
       -${{ hashFiles('**/Cargo.lock', '.cargo/config.toml') }}
  restore-keys: （去掉 hashFiles 那段）
  ```

  恢复 `target/`（仅同 profile/feature 目录）+ `~/.cargo/registry` + `~/.cargo/git`。
- **正确性优先于命中率**：命中率高 ≠ 正确。必须能回答「同一 commit 连续两次
  run 是否稳定 Fresh」「单行 `.rs` 改动有没有重编 `quire-core` 的稳定中间产物」。
  命中率 < 30% 先查 key 是否过细或 lock 是否每 run 变。
- **sccache 只做本地只读试点**：跑 20 次不同 commit 看命中率与**产物哈希一致性**。
  单机开发优先保 `target/` 的本地 incremental。

### L3 · 重构（只有基准证明边界稳定才动）

**先拆 `quire-platform`，再评估 `quire-ui`**；`shell controller/state` 留在
`quire`；`quire-core` 已在外部仓库，不要重复搬运。依赖方向必须单向
（`ui -> core`、`platform -> core`），**先消除反向引用，再迁移**。

`slint::include_modules!` 会把 UI 树变成单 crate 的生成代码，所以 UI crate 只能
对外暴露**命令 / 事件 / 视图 model**，由 shell 实现适配器。目标：改颜色/布局/动画
不触发领域逻辑重编；领域内部重构不重编 UI 生成代码。

**验收（拆分的唯一正当理由）**：

| 改动 | 拆分前 | 拆分后必须满足 |
|---|---|---|
| 仅改 Windows adapter | 整个 `quire` 重编 | 只 `quire-platform` 重编 |
| 仅改 `ui/Colors.slint` | Rust 领域逻辑无故重编 | 仅 UI crate 重编 |
| 改 controller 内部实现（不动公开 API） | 下游全重编 | 下游不重新 codegen |

若拆分后单文件 dev 构建不降、跨 crate 接口天天变、测试复杂度上升 →
**停止拆分，回到 L1**。

**`quire-resources` 独立 crate**（原方案的 6.4）：把 `.rc` 的生成搬进去，
主 crate 只链接。收益是版本 bump 时 `quire` 逻辑 crate 保持 Fresh。**只在
「发版时的全编是主要抱怨」时才值得** —— 它增加构建复杂度，而当前的发版频率
下这个抱怨并不成立。优先级低于拆 `platform`。

**远程对象缓存 / build farm**：本仓库单机开发，**不适用**。成立条件是内容寻址
key 稳定 + 多 runner；一旦出现「缓存命中但二进制功能异常」立即禁写回滚。

## 6. 执行顺序

1. **P0** · 建基线（§7 的方法）—— 没有基线，任何「快了」都是噪声。
2. **P1** · 加 `rust-toolchain.toml` + 提交 `Cargo.lock`（L1-B）。
3. **P2** · 明确写下「`build.rs` 的 include graph 由 `slint-build` 提供，不要
   手写遍历」这条约定，避免下一次又有人来「优化」它（ADR-0148）。
4. **P3** · 若重引 CI → L2 三门分离 + 分桶缓存。
5. **P4** · 只有当 §5-L3 的验收表被基准证实时才拆 `platform`；`ui` 再看。
6. **不做** · sccache / 远程缓存 / `quire-resources`（单机开发，无对应收益）。

## 7. 怎么测（方法，不是结论）

固定机器、commit、电源模式、toolchain，热机 30 秒，连续 5 次：

1. 无改动 `cargo build --release`
2. 仅改 `src/app/state.rs`
3. 仅改 `ui/AppWindow.slint`
4. 仅改 `ui/Colors.slint`
5. 仅 bump `Cargo.toml` 的 patch

每次记录：wall time、Fresh/Dirty 摘要、最终链接时间、峰值内存、`target/` 大小。
报告均值 / p50 / p95，**只跟同一 batch 比 delta，不跨 batch 比** ——
PERFORMANCE.md 自己的 drift 记录已经证明跨 session 的读数会漂 10 MB 量级。

「touch 一个文件」的写法与 PERFORMANCE.md 一致：
`(Get-Item src\main.rs).LastWriteTime = Get-Date`。
输出走 `cmd /c` 重定向而非管道（`powershell -Command` 会把原生命令的 stdout
按控制台代码页重新编码，并且 `$ErrorActionPreference='Stop'` 会把 cargo 的
stderr 当成失败）。

## 8. 决策树

- **无改动构建 > 10 s 且依赖在重编？** → 查 lock、toolchain、git fetch、feature
  是否变了。**不是拆 crate。**
- **无改动 < 6 s（现在是 4.4 s）？** → 增量健康，问题在单 crate 粒度，见 R1。
- **改 `.slint` 会重跑 Slint 前端吗？** → 会，而且**应该**会：那张 include graph
  就是它的输入。**不要去「修」它**（§3）。
- **dev > 90 s、CPU 未饱和、内存有余？** → 调 `-j`，继续查单 crate 内生成代码量。
- **release 每改一行都 2 分 30 秒？** → **接受这是 `cgu=1 + thinLTO` 的结构代价**。
  要动只能是 L3 拆 crate，且要先有基准。
- **CI restore hit 高但测试偶失败？** → 暂停缓存写入、清相关缓存、查
  非确定 `build.rs`、时间/路径输入。

## 9. 尚未验证

诚实地划出这份文档的边界：

- **视觉回归 sweep 未完成，不能作为本次改动的证据。** 跑出来的 186 个场景与最大的
  可比基线 `sweep-align2`（173 场景，commit `17277d6`，2026-09-27）**173 个全部
  不同**，另有 13 个场景基线里没有（notes/tasks 区域、sync 页）。这个差异来自
  9-27 至今的其他工作，**与本次改动无关**（本次只动 docs、justfile 注释与
  `rust-toolchain.toml`，一行 UI 代码都没碰），但它意味着**没有可用于本次的
  干净基线**，所以「sweep 无差异」这句话这次说不出来。
  另有一个环境污染：本机运行中的实例占着 sync 端口，
  `dark-sync` / `dark-sync-log` / `dark-sync-stats` 三个场景拍到的是同一张降级图
  （三者哈希全等 `B580DB4D…`）。**要做视觉验收，先退出运行中的实例，并把
  `sweep.ps1` 的场景集更新到当前 HEAD。**
- **§2 的 release 复核是一次运行的读数**（209 s / 11.2 s / 8.5 s），不是多轮中位数。
  PERFORMANCE.md 的 4.4 s / 2 m 30 s / 3 m 07 s / 7 m 59 s 全部**转引** 2026-09-25
  的测量，本次没有重跑。跨 10 天的读数按本文件自己的规矩不作跨 batch 比较。
- **`cgu16` 在当前 HEAD 上是否仍然慢**，未复测（沿用 2026-09-25 的 3 m 07 s）。
  该仓库此后新增了大量 `.slint` 与 Rust 代码，单 crate 已经从 113 MB rlib 的规模
  长到现在这个规模，所以 209 s 那行的方向可信、**幅度不可外推**到 9-25 的 150 s。
- **`Cargo.toml` / `Cargo.lock` 改动是否真的会重跑 Slint 前端**，未测。
  它们不在那张 include graph 里，而 build.rs 不读它们 —— 按 cargo 的语义
  它们会重编 crate 但不一定重跑 build script。这是 §3 留下的一个真实缺口。
- **L1-C 的 `CARGO_BUILD_JOBS` 收益**，未测。按 §5 的推理它只作用于多 crate 阶段。
- **L3 全部未验证**，包括「拆了到底能省多少」这个最要紧的数字。§5 的验收表是
  **待测量的假设**，不是预测。
- **PERFORMANCE.md 提出的「codegen 与 link 的时间拆分从未被测过」这一条已经关闭**
  —— §2 给出 198 s / 11.2 s。**未定价的杠杆因此只剩「拆 crate」**，而它属于 L3。

## 10. 相关文件

- `docs/PERFORMANCE.md` *Build cost (2026-09-25)* —— 转引数字的出处
- `docs/DECISIONS.md` ADR-0024 —— release profile 审计（`cgu1 + thin` 的由来）
- `docs/DECISIONS.md` ADR-0134 —— 磁盘治理、`just sweep`、多 target-dir
- `docs/DECISIONS.md` ADR-0148 —— `build.rs` 的 include graph 交给 `slint-build`
- `rust-toolchain.toml` —— L1-B 的落地，钉住测量所依赖的编译器
- `docs/LIVE_PREVIEW.md` —— 视觉迭代回路（`build.rs` 重跑的另一条动机，与本文
  的增量边界是同一个问题的两面）
- `justfile:16-39` · `build.rs` · `Cargo.toml:87-141`

## 11. 复盘：0.1.27 patch build 与 22 GiB `target/`（2026-10-17）

这次发 patch 的记录在 `README.md`。这里补上构建过程中踩到的坑，避免把
重复编译时间或整个 `target/` 的旧缓存误算成本次 release 的单次代价。

### 为什么编译了两次

最终交付前有 **两次能确认完成的** `cargo build --release`：第一轮 4m53s，第二轮
9m53s，共 14m46s。第一轮完成后，又把窗口、任务栏和托盘标识从
`quire-0.1.27(slint)` 改成用户指定的 `quire-slint-0.1.27`，这些字样包含编译期
Rust 字符串，所以最终二进制必须再编一次。第二轮耗时比第一轮长；单次运行会受
机器负载和构建波动影响，不能拿它证明源码改动本身令编译变慢。

在两轮确认构建前，还有一次后台构建/测量尝试：命令把 Cargo 放进 PowerShell
后台 Job，之后在同一个脚本里递归扫描整个 `target/`。扫描大目录时脚本迟迟没有
返回，最终没有保存 Cargo 的完成输出、可靠计时或可验证的 0.1.27 产物。因此这次
尝试不计入 14m46s；README 的缓存前后差值从尝试前开始量，可能包括它写入的产物。
**坑：不要让昂贵构建与全目录递归测量串在一个后台 Job 中**。分开记录构建进程
的退出码与墙钟时间；构建完成后再按 `target/debug`、`target/release` 等具体目录
统计磁盘占用，并把原始结果落盘。

**改进顺序：**先定下产品名、版本格式及窗口/托盘/安装目录的所有展示位置，再
修改代码；先用搜索核对全部标识，再开始 patch build。这样避免“先编译、后统一
命名”导致整 crate 重建。

### 22 GiB 是什么

构建后复测的 `target/` 为 **24,017,763,693 B（22.37 GiB）**。分布如下：

| 目录 | 大小 | 含义 |
|---|---:|---|
| `target/debug` | 19.53 GiB | 历史开发构建产物 |
| └ `incremental` | 11.74 GiB | 多个 Rust 增量编译 session 的保留状态；最大的七个 `quire-*` 子目录约 0.39–2.47 GiB 各一份 |
| └ `deps` | 5.36 GiB | `.rlib`、`.rmeta`、PDB、开发用 EXE 等；仍有两份约 467 MiB 的 `libquire` rlib |
| └ `build` | 1.02 GiB | 依赖的 build-script 输出与中间产物 |
| `target/release` | 2.84 GiB | Release 依赖、中间产物、链接产物和最终 EXE |

所以 **22 GiB 不是一次 Release 构建写出来的**。本次构建前同一棵 `target/` 已有
23,706,667,601 B（22.08 GiB）；到最终复测净增 **311,096,092 B（约 296.6 MiB）**。
本机 `Cargo.toml` 的 dev profile 已设 `debug = 1`，但这不会自动删除过去积累的
增量 session，也不会合并不同 fingerprint / feature set 的产物。release patch
构建主要更新 `target/release`；22 GiB 的主体是 `target/debug` 的历史状态。

空间治理时先看 `target/debug/incremental` 是否仍需保留来加速当前开发。不要仅因
整个 `target/` 看起来很大就做无差别清理：清掉活跃 profile 的增量缓存会把下一次
开发构建变成冷构建；不同 renderer/feature 的目录边界也有意隔离（ADR-0134）。
先记录各子目录大小与用途，再决定清理哪一份。

### 以后记数的边界

- 编译时间记 Cargo 墙钟耗时、退出码、命令、profile、feature/renderer 与源码状态。
  “最后一次成功 build”不等于“本次所有尝试耗时”。不完整或没有退出码的后台尝试
  单列为未确认，不能混进成功构建均值。
- 缓存占用用同一组明确目录做前后快照；把 `target/` 总量与本次新增量分开写，
  不把整个历史目录称为本次编译产物。
- 编译峰值内存需在 Cargo 子进程运行期间采样。事后仅凭 EXE 或缓存大小推不出来；
  没采到就留空，不用另一轮或另一构建类型的数据代替。
- 旧数据与本次数字只有在机器、profile、缓存冷热状态及命令相近时才可直接比较。
  README 中 GPUI 的 55.03s 是增量 Release build；Slint 的 14m46s 是两个确认完成的
  patch Release build，构建类型不同，必须保留这个限定。

## 12. dev 档案的依赖改成优化编译：一次 1040 s，之后不变（2026-10-20，ADR-0168）

第 7 行那句「`just deploy-workshop` 现在指 dev 档案那一条，几秒钟就完」在 2026-10-20 之后要加一个
限定：**几秒钟说的是改动之后的增量构建**，改 profile 的那一轮本身要先付一次全树重编。

`Cargo.toml` 加了 `[profile.dev.package."*"] opt-level = 2, debug = 0`（理由与逐臂帧数在
PERFORMANCE.md 的 ADR-0168 一节：侧栏 220 ms 补间在未优化依赖下只画 1 帧）。按上面「以后记数的边界」
那四条记录：

| 命令 | profile / features | 树的状态 | 墙钟 | 退出码 |
| --- | --- | --- | ---: | --- |
| `cargo build --bin quire-slint` | dev / femtovg（默认） | 改 profile 后，依赖全冷 | 1040 s | **101** |
| `cargo build --bin quire-slint` | 同上 | 依赖已绿，只差 `quire` | 12 s | 0 |
| `cargo build --bin quire-slint` | 同上 | 动 `.rs` 的常态增量 | 11–15 s | 0 |
| `cargo check --workspace --all-targets` | dev / 全 targets | 242 个 unit 重跑 | 205 s | 0 |

那一次 101 **不是构建问题**：1040 s 里 325 个依赖 crate 全部编完，红在最后一步 `quire` 自己身上，
因为同一个文件正在被编辑（探针的一处 `ModelRc::len`）。按规矩它记作失败尝试，不进成功均值；绿的
是紧随其后的 12 s 那一次。

产物与磁盘：exe 从 85,958,144 缩到 67,369,472 字节（依赖不带 debuginfo），`target/debug` 净多约
3 GB（旧指纹的单元不会被回收，ADR-0134 那件事的小号版本）。C: 空闲从 38 GB 掉到 35 GB。

没有新增第三个 profile。`just check` 注释里记的陷阱——换 profile 会作废整张 unit graph——正是这里
的 1040 s；把它做成 `dist` 档案等于让 dev、dist、release、target-shot 四棵树并存，所以选择让 dev
这一棵树自己把依赖编快，四棵树还是一棵都别多。
