# 全模型思考档位无条件开放 设计文档

日期：2026-11-19
状态：待批准

## 意图与动机

用户要求：所有模型在 Codex 和 Claude Code 两个客户端里都能选到 `xhigh`、`max` 等深档，不论模型属于哪个家族。

今天两条链路各自按"模型能力猜测"收窄档位，导致中转站上的 `glm-*`、`kimi-*`、`qwen-*` 以及 Luna / Gemini 等家族的深档根本不出现在菜单里。用户的诉求是"选什么就发什么"，由上游自己决定接不接受，而不是由网关提前猜。

## 目标

- Codex 的 `supported_reasoning_levels` 对每个模型都给出 `none/low/medium/high/xhigh/max`。
- 网关 `/v1/models` 的 `capabilities.effort` 对每个模型都把 `xhigh`、`max` 报为 supported。
- 网关转发时原样透传 effort，不再按模型家族降级成 `high`。
- 删掉基于模型名的能力猜测函数，减少维护面。

## 非目标

- 不实现"上游拒绝后自动降级重试"。用户明确选择原样透传，400 就是 400。
- 不改 Claude Code 客户端自身的档位过滤逻辑（那是二进制内置行为，PolyDeck 只控制它读取的 env 与网关响应）。
- 不改 `max` 通过 `output_config.effort` 发送的形态（见既有记忆 `claude-code-effort-max-behavior`）。

## 约束

- 自动思考模型（`reasoner` / `r1` / `qwq`）发 effort 会被上游确定性拒绝，这是协议事实不是能力猜测，保留丢弃逻辑。
- 上游在 `/v1/models` 里**明确**声明 `capabilities.effort.supported=false` 时，网关不改写它。只对"缺省能力元数据"的模型补全档位。
- 本机 claude.exe 2.1.327 反编译确认（`_Ri()` 解析函数、`wDi` 能力名表）：

```js
// 解析：分号分隔条目，`键=能力,能力`；键以 * 结尾做前缀匹配，无 = 则对全部模型生效
function _Ri(e){ let t=new Map(), o=s.CLAUDE_CODE_MODEL_CAPABILITIES; ... }
// 内置能力名（节选）：["effort","max_effort","xhigh_effort","adaptive_thinking",...]
```

  `claude-haiku-4-5` 的内置目录条目是
  `capabilities:["context_management","total_tokens_reminder","tool_loaded_text"]`
  —— 连 `effort` 本身都没有，所以 haiku 档在 Claude Code 里连档位选择器都不出现。仅靠网关 `/v1/models` 无法解决这一档（内置目录优先于发现结果），因此需要写强制 env。

## 方案对比

### 方案 A（选定）：删除能力猜测，全档位无条件开放

删掉 `supports_extended_reasoning_effort`、`upstream_serves_max_effort` 两个函数及所有调用点，档位表统一为六档，转发不降级。

- 优点：代码净减，没有家族表要维护，上游升级能力自动生效，行为符合用户"原样透传"的诉求。
- 缺点：Luna / Gemini / 非推理模型上选深档会收到上游 400，属于用户已知并接受的代价。

### 方案 B（排除）：只往现有家族表里补档位

把 Luna、Gemini、fallback 都补到六档，但保留家族判断表和 `supports_extended_reasoning_effort`。

- 排除原因：本质仍是能力猜测，新增模型还要再改一次表，且用户要求"无条件"，保留判断表与目标矛盾。

### 方案 C（排除）：加 profile 级"全档位透传"开关

- 排除原因：用户明确说无条件全开，加开关只会多一层状态和一套默认值语义，没有对应需求。

## 具体改动

### 1. `crates/core/src/types.rs`
删除 `supports_extended_reasoning_effort`（及其文档注释）。全仓仅 5 处引用，全部在本次改动范围内。

### 2. `crates/core/src/profile_switch/codex_catalog.rs`
- `get_model_reasoning_config` 重写为：非推理模型（gpt-4o / gpt-3.5 / claude-3-5 / deepseek-chat / deepseek-v3 / glm-4 / llama 等）与其余所有模型共用同一张六档表，默认档统一 `high`。
- 删除 `fallback_reasoning_levels()`（不再有"保守三档"这个概念）；保留 `sol` / `terra` / `luna` / Claude / gpt-5 / gemini / o1·o3·o4 等分支会与统一表重复，一并删除。
- 返回的第三项 `supports_reasoning` 统一为 `true`（原来非推理模型返回 `false`，现在它们也能选档位）。`none` 档仍可用于关闭思考。
- `model_window()` 与 `codex_context_window()` 不动——那是上下文窗口，与档位无关。

### 3. `crates/gateway/src/router/models.rs`
- 删除 `upstream_serves_max_effort`、`EFFORT_SUFFIX_SEPARATORS`。
- `synthesize_capabilities` 去掉 `serves_max` 参数，`effort.max.supported` 恒为 `true`。
- `synthesize_models_response` 删除 `serves_max` / `model_serves_max` 计算与"已有 capabilities 时补 max"分支。
- **已有 capabilities 时的处理**（用户已决：尊重上游的 false）：
  - 模型完全没有 `capabilities` → 整体写入 `synthesize_capabilities(...)`（六档全 true）。
  - 模型有 `capabilities` 但不是对象 → 同上整体替换。
  - 模型有 `capabilities` 对象 → **在原对象上补缺**：缺失的档位键补成 `{supported:true}`；上游明确写了的键（`true` 或 `false`）一律保留。`thinking.supported` 仍按探测结果覆盖（既有行为，与档位无关）。

### 4. `crates/gateway/src/router/effort.rs`
- `EFFORT_WHITELIST` 不变（本来就是五档 + none 处理）。
- `normalize_effort` 保留：`minimal` → `low` 是协议对齐不是能力猜测；`none/off/0/false` 丢弃是语义；未知值丢弃是防 400 的既有契约。
- 自动思考模型的丢弃逻辑保留。

### 5. `crates/core/src/messages_chat.rs`
- `normalize_message_effort` 删除 `xhigh | max if supports_extended_reasoning_effort` 分支，改为 `xhigh | max` 直接放行，与 low/medium/high 同臂。
- `effort_from_request` 里由 `budget_tokens` 反推 effort 的降级分支一并删除（`budget_to_effort` 保留）。
- **新增 `output_config.effort` 读取**（用户已决：补上转换）。Claude Code 从不发 `reasoning_effort`，max/xhigh 是以 `output_config: {effort:"max"}` 的形式发出的（见记忆 `claude-code-effort-max-behavior` 的 stub 实测）。当前桥接路径只读 `reasoning_effort` 和 `thinking.effort`，所以走 Chat 桥接的中转站上选 max 会静默丢失。
  读取优先级（从高到低）：
  1. `reasoning_effort`（显式 OpenAI 风格）
  2. `thinking.effort`
  3. `output_config.effort`（新增）
  4. `thinking.budget_tokens` 反推
  5. `fallback_effort`
  
  同时把 `output_config` 从转发体里剥掉：Chat Completions 不认识这个字段，原样带着会让部分严格上游 400。

### 6. `crates/core/src/responses_chat.rs`
- `responses_to_chat` 中的 `supports_extended` 判断删除，`xhigh`/`max` 与 low/medium/high 同臂透传。
- Gemini 分支的 `xhigh|max → high` 降级：按"所有模型无条件全开"删除，Gemini 也原样透传（用户已确认统一处理，不做家族例外）。

### 7. `crates/core/src/profile_switch/mod.rs` 写入强制能力 env
- 在 `write_claude_config` 的 env 块增加：

```
CLAUDE_CODE_MODEL_CAPABILITIES = *=effort,max_effort,xhigh_effort
```

- 只在 `gateway_enabled` 时写入。关闭网关时删除该键（同 `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY` 的写法：两分支都要处理，因为 settings.json 是合并不是替换）。
- 依据：`_Ri()` 的键匹配规则里 `*` 结尾走前缀匹配，`*` 单独即是空前缀，命中所有模型；无 `=` 的条目对所有模型生效。写入 `effort` 本身是必须的——haiku 档内置目录连 `effort` 都没有，不补这一项选择器根本不出现。
- 不写 `-xxx` 否定项：默认全放行即可，不需要额外收紧。

### 8. 前端 `src/pages/ProfilesPage.tsx`
思考深度下拉已经是六档（`none/low/medium/high/xhigh/max`，`:2213-2218`），无需改动。

### 9. 文档
- `CHANGELOG.md` 的 `[Unreleased]` 加一条说明。
- `README.md` 第 40-42 行的 Sol/Terra/Luna 档位描述已过时（说 Luna 最高 high、Terra 最高 xhigh），改为"所有模型统一六档，档位由上游决定是否接受"。

## 受影响的既有测试（需同步改写）

| 测试 | 位置 | 改法 |
|---|---|---|
| `unknown_third_party_models_get_the_safe_three_level_ladder` | `profile_switch/mod.rs:1298` | 改为断言六档 |
| `deepseek_v4_models_offer_the_extended_effort_ladder` | `:1315` | 保留，但改为通用于任意模型名 |
| `non_reasoning_models_offer_none_rather_than_nothing` | `:1337` | 改为断言六档、默认 high、`supports_reasoning=true` |
| `test_build_codex_catalog_structure` | `:1206` | Luna/Gemini 档位数从 4/3 改为 6 |
| `every_model_advertises_at_least_one_reasoning_level` | `:1255` | 保留（默认档须在列表内仍成立） |
| `normalize_effort_*` 系列 | `router/mod.rs:1997-2038` | 基本不变，`bogus` 丢弃行为保留 |
| `max_effort_detection_spans_suffix_spellings` | `router/mod.rs:2172` | 删除（函数已不存在） |
| `max_effort_not_inferred_from_product_names` | `:2193` | 删除 |
| `max_effort_offered_when_upstream_advertises_a_max_model` | `:2156` | 改为"任何模型都报 max supported" |
| `synthesize_models_response_adds_max_to_deepseek_v4_existing_capabilities` | `:2230` | 改为"缺省档位键补齐、上游显式 false 保留" |
| `synthesize_models_response_preserves_existing_capabilities` | `:2222` | 保留，断言 `effort.supported=false` 不被改写 |
| `clamps_extended_effort_for_models_that_do_not_support_it` | `messages_chat.rs:982` | 改为断言原样透传 max |
| `downgrades_extended_reasoning_effort_for_chat_backends` | `responses_chat.rs:1035` | 改为断言原样透传 xhigh |
| `maps_reasoning_effort_levels` 中的 Gemini/降级断言 | `responses_chat.rs:1385-1423` | Gemini 分支与通用降级断言改为透传 |
| `thinking_discovery_wire.rs` 的 `assert_eq!(expected)` 逻辑 | `:2126` 附近 | 别名与原始 id 都应为 true |

新增测试：

| 测试 | 位置 | 断言 |
|---|---|---|
| `output_config_effort_reaches_the_chat_body` | `messages_chat.rs` 测试模块 | `{"output_config":{"effort":"max"}}` 桥接后 `reasoning_effort == "max"`，且 `output_config` 不出现在转发体 |
| `output_config_does_not_beat_an_explicit_reasoning_effort` | 同上 | 两者同时存在时以 `reasoning_effort` 为准 |
| `claude_env_forces_full_effort_capabilities_behind_the_gateway` | `profile_switch/mod.rs` 测试模块 | 网关开启时 `CLAUDE_CODE_MODEL_CAPABILITIES` 有值；关闭时该键被删除 |

## 验收

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run lint && npx tsc --noEmit
npm test
```

全部通过后，实际行为验证：网关 `/v1/models` 返回的每个模型 `capabilities.effort.{xhigh,max}.supported` 都是 true；Codex 写入的 `model_catalog_json` 里每个模型 `supported_reasoning_levels` 都是六档。

【无法确认】未做真实 Codex CLI / Claude Code 客户端 + 真实中转站的端到端联调，这一步需要用户装包实测。

## 留给实施者的决定

无。所有分支判断已在本文件写明。
