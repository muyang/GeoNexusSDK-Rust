<!--
This repository is generated from the GeoNexus monorepo (muyang/GeoNexus-SDK),
subdirectory `sdk-rust/`, at commit 21c6af7. Edits made here must be carried back to
the monorepo, otherwise the two diverge silently.

The vendored `schemas/geocard.schema.json` comes from the same upstream commit
and is embedded at compile time; refresh it with
`./scripts/sync-schema.sh /path/to/GeoNexus-SDK`.
-->

# GeoNexus SDK for Rust

Rust 客户端，对齐 **GeoNexus-SDK v1.1.1（Python）的内核层**。
**零依赖**（仅 `std`）。

> ## ✅ 验证状态：编译、clippy、测试全部通过
>
> ```
> rustc 1.99.1
> cargo build                    ✅
> cargo check --all-targets      ✅   （测试代码也一并类型检查）
> cargo clippy --all-targets     ✅   零警告
> cargo test                     ✅   42 个单元测试 + 2 个文档测试
> cargo doc --no-deps            ✅
> ```
>
> 一条命令复跑：`./scripts/verify-rust.sh`
>
> ### 过程中遇到的两个环境问题（脚本已处理）
>
> 1. **装不上工具链**：`crates.io` / `static.crates.io` 返回 403，
>    `static.rust-lang.org` 实测约 21 KB/s，完整工具链约 200 MB。
>    最终等待完成安装（约 50 分钟），此后一直可用。
> 2. **链接器坏掉**：本机 `/usr/bin/cc` 是转调 `xcrun` 的壳，而
>    `/Library/Developer/CommandLineTools` 不存在，于是
>    `cargo test` 在**链接阶段**失败（编译是好的）。系统里另存有一份 SDK，
>    脚本据此用 `ld64.lld` + `-syslibroot` 组装链接命令，把
>    gcc 风格参数翻译过去。
>
> 脚本还额外处理了两个容易踩的坑：
>
> 3. **工具链放在哪**：脚本从仓库目录向上探测已有的 `.rust-toolchain/`，
>    找不到才在该仓库内安装——否则会误用一个残缺副本（实测报
>    `missing manifest in toolchain`）。
> 4. **没有默认工具链**：`rustc` 只是 rustup 的代理，没有默认工具链时会拒绝运行
>    并给出很长的提示；脚本检测到就自动 `rustup default stable`。
>
> 换一台正常机器，以上分支都会被跳过，直接走默认路径。
>
> ### 验证中发现的真实问题（已修）
>
> 两条测试用例预先计算请求 id，而客户端的 `next_id()` 会自增，导致差一；
> 测试改为让 mock 回显请求里的 id（与 TypeScript 版一致）。
> 这是**测试的缺陷**，不是实现的缺陷——但它证明了这些测试确实在跑。

## 覆盖范围

| 模块 | 职责 | 对应 Python |
|---|---|---|
| `json` | JSON 值、解析、序列化 | `json` |
| `error` | 错误类型与两段错误码 | 异常类 |
| `transport` | 可注入的 HTTP（std 实现 + Mock） | `httpx` 用法 |
| `geocard` | 契约校验、构建器、严格/宽松分层 | `geonexus.geocard` |
| `geomcp` | JSON-RPC 信封、方法表、错误码、客户端 | `geonexus.geomcp.client` |
| `registry` | 注册、检索、审核、节点发现 | `geonexus.registry.client` |

**刻意不做**：OGE 适配器、Agent 规划、任务图、配方引擎、GAAG、联邦下推。
适配器按设计处在边缘（见 `docs/BOUNDARIES.md`）；在无法编译验证的前提下
把它们一并"交付"，得到的只会是未经证实的声明。TypeScript 版带 OGE 适配器，
Rust 版只覆盖内核。

## 为什么零依赖

1. **供应链最小化**：嵌进别人的服务时，不因为一个 HTTP 客户端拖进几十个传递依赖。
2. **现实约束**：本项目的构建环境访问不到 crates.io，加依赖就编译不了。

代价很明确：`std` 没有 TLS，所以默认的 `StdHttpTransport` **只支持 `http://`**，
遇到 `https://` 会给出明确错误而不是静默失败。本项目的 GeoMCP 节点、注册中心
与 OGE 都是明文 HTTP，够用。

需要 HTTPS 时实现 `Transport` 即可——协议与卡片逻辑完全不依赖传输层：

```rust
use geonexus::{transport::{HttpRequest, HttpResponse, Transport, HttpError}, GeoMcpClient};

struct MyTlsTransport { /* your client */ }

impl Transport for MyTlsTransport {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        // 用你选的 HTTP 库发出 request，返回 status + body
        # unimplemented!()
    }
}

let transport = MyTlsTransport { /* ... */ };
let node = GeoMcpClient::new("https://node.example", &transport);
```

## 用法

```rust
use geonexus::{json::Json, transport::StdHttpTransport, GeoMcpClient};

let transport = StdHttpTransport::default();
let node = GeoMcpClient::new("http://127.0.0.1:8787", &transport);

let caps = node.capabilities()?;
println!("{}", caps.to_string_pretty());

let result = node.execute(Json::object([
    ("skill", Json::string("oge-ndvi")),
    ("params", Json::object([
        ("coverage", Json::string("Platform:Coverage:img:prod")),
    ])),
]))?;
```

### 卡片校验：严格与宽松分开

```rust
use geonexus::{geocard::{parse_card, validate_card, GeoCardBuilder}, json::Json};

let card = GeoCardBuilder::new("asset-1", "data", "Asset", "desc")
    .spatial(vec![100.0, 20.0, 110.0, 30.0], "EPSG:4326")
    .capability("ndvi")
    .build()?;

// 检视模式：不查契约，只做结构解析——用来读别人给的坏卡
let loose = parse_card(card.clone())?;
```

与 Python 的 `load_geocard(validate=False)`、TypeScript 的 `parseGeoCard` 行为一致。

### 错误码分两段

负数 = JSON-RPC 2.0 保留码（不可自造）；2xxx = GeoNexus 应用码。
测试会断言两段不相交——混淆会让客户端无法区分"协议层失败"与"业务拒绝"。

```rust
match node.execute(params) {
    Err(geonexus::Error::GeoMcp(err)) => {
        println!("{} {}", err.code, err.category()); // e.g. "2001 skill-not-found"
        if err.is_protocol_level() { /* 负数：传输/信封问题 */ }
    }
    other => other?,
}
```

## 契约不会漂移

`schemas/geocard.schema.json` 通过 `include_str!` 在编译期内嵌，并由一个测试
用本 crate 自己的 JSON 解析器解析它、断言 `GEOCARD_TYPES` 与 schema 的
`enum` 完全一致。所以 Rust 版无法在无提示的情况下偏离已发布契约——若偏离，
测试失败。

## 测试

```bash
cargo test
```

用例覆盖：JSON 解析/序列化的边界（转义、代理对、非法数字、尾逗号）、
信封校验的每条失败路径、卡片校验规则、Registry 的 URL 构造与错误映射、
以及 transport 对 `Content-Length` / `chunked` 两种响应的解析。

单元测试用 `MockTransport`，不触网。

## 已知问题

除"未被编译验证"之外：

- `StdHttpTransport` 不支持 HTTPS、不跟随重定向、不复用连接。目标是够用且可审计。
- `card_errors` 只实现了 schema 实际使用的约束（必填、type 枚举、id 模式、
  bbox 元数、capabilities 形态）。它不是通用 JSON Schema 引擎——但也不假装是：
  没有"忽略未知关键字"的行为，因为那类校验器会回报 valid 而实际没校验。
