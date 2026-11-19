# 全模型思考档位无条件开放 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让每个模型在 Codex 和 Claude Code 里都能选到 `xhigh`、`max`，转发时原样透传不降级。

**Architecture:** 删掉两处基于模型名的能力猜测（Codex 的家族分级表、网关的 `-max` 后缀推断），两处统一改为「六档全给」；桥接路径补读 Claude Code 实际使用的 `output_config.effort`；网关开启时写一个强制能力 env 覆盖 Claude Code 内置目录对 haiku 档的封锁。

**Tech Stack:** Rust workspace（core / gateway / inject）、Tauri 2、React 19、tokio、serde_json、toml_edit

**Spec:** `docs/superpowers/specs/2026-11-19-all-models-full-effort-ladder-design.md`

## Global Constraints

- 档位顺序固定为 `none / low / medium / high / xhigh / max`，中文描述沿用 `codex_catalog.rs` 现有文案。
- `get_model_reasoning_config` 返回三元组 `(default, levels, supports_reasoning)`，默认档一律 `"high"`，第三项一律 `true`。
- 上游在 `/v1/models` 显式写 `capabilities.effort.supported=false` 时**不得改写**；只补缺失的档位键。
- 名字含 `reasoner` / `r1` / `qwq` 的自动思考模型，丢弃 effort 的逻辑保留。
- 强制 env 精确值：`CLAUDE_CODE_MODEL_CAPABILITIES=*=effort,max_effort,xhigh_effort`
- 网关只绑环回，不得改动。

## Review Focus

规格没逐条写、但用起来一定碰到，按最可能出问题的顺序，每条都在对应任务的测试里钉住：

1. 上游模型自带 `capabilities` 对象 → 只补缺，上游写的 `false` 保留（任务 4）。
2. 同时带 `output_config.effort` 和 `reasoning_effort` → 以 `reasoning_effort` 为准（任务 6）。
3. 关网关后残留的旧 env → `CLAUDE_CODE_MODEL_CAPABILITIES` 必须被删除（任务 7）。
4. `output_config` 混进 Chat 转发体 → 必须剥掉（任务 6）。
5. 自动思考模型收到 effort → 丢弃而非透传（任务 2、任务 3；网关侧由既有用例 `normalize_effort_drops_for_autothinking_models` 覆盖，不动）。

## 文件结构

| 文件 | 职责 | 动作 |
|---|---|---|
| `crates/core/src/profile_switch/codex_catalog.rs` | 生成 Codex 模型目录（档位表） | 改统一六档，删家族分支 |
| `crates/core/src/profile_switch/mod.rs` | 方案写入 Claude 配置 | 加强制能力 env |
| `crates/core/src/messages_chat.rs` | Messages → Chat 桥接 | 透传深档、读 `output_config.effort` |
| `crates/core/src/responses_chat.rs` | Responses → Chat 桥接 | 透传深档 |
| `crates/core/src/types.rs` | 共享类型 | 删 `supports_extended_reasoning_effort` |
| `crates/gateway/src/router/models.rs` | `/v1/models` 能力合成 | 删后缀推断，改补缺 |
| `crates/gateway/src/router/mod.rs` | 路由与测试 | 同步测试 |
| `crates/gateway/tests/thinking_discovery_wire.rs` | 接线测试 | 不需改动（测的是 thinking 开关，与档位无关） |
| `README.md`、`CHANGELOG.md` | 文档 | 更新档位描述 |

任务顺序按依赖排：先改 Codex 目录（完全独立），再改两条桥接路径，然后网关能力合成，最后强制 env 与文档。每个任务结束都要能独立编译并通过自己的测试。

---

### Task 1: Codex 目录统一六档

**Files:**
- Modify: `crates/core/src/profile_switch/codex_catalog.rs:11-256`
- Test: `crates/core/src/profile_switch/mod.rs:1206-1350`（`mod tests` 内的既有用例）

**Interfaces:**
- Consumes: 无（本任务改的是最底层）
- Produces: `pub(super) fn get_model_reasoning_config(slug: &str) -> (serde_json::Value, serde_json::Value, bool)` — 签名不变，语义变为「任何 slug 都返回六档表、默认 `high`、第三项 `true`」。`build_codex_catalog_with_1m` 的调用点 `:314` 无需改动。

- [ ] **Step 1: 改写既有测试，先把新期望写进去**

在 `crates/core/src/profile_switch/mod.rs` 的 `mod tests` 内，把这三个用例改成：

```rust
#[test]
fn every_model_gets_the_same_full_six_level_ladder() {
    for name in [
        "qwen3.8-max",
        "brand-new-model",
        "gpt-5.6-luna",
        "gemini-2.5-pro",
        "deepseek-v4-pro-0813",
        "claude-opus-5",
        "gpt-4o",
        "deepseek-chat",
        "llama-3.3-70b",
    ] {
        let (default, levels, supports) = get_model_reasoning_config(name);
        assert!(supports, "{name} 必须声明支持推理摘要");
        assert_eq!(default, "high", "{name} 的默认档必须是 high");
        let efforts: Vec<&str> = levels
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["effort"].as_str().unwrap())
            .collect();
        assert_eq!(
            efforts,
            vec!["none", "low", "medium", "high", "xhigh", "max"],
            "{name} 必须拿到完整的六档表"
        );
    }
}
```

把 `test_build_codex_catalog_structure` 里三处断言改掉：

- `:1214-1219` 的 `list[0]["supported_reasoning_levels"].len() == 3` 改为 `== 6`（`gemini-3.7-flash-high` 现在也拿六档）。
- `:1231-1254` 对 Sol/Terra/Luna/Gemini 档位数的整段断言替换为：

```rust
        // 每个模型都是同一张六档表，不再按家族收窄。
        for name in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gemini-2.5-pro"] {
            let (default, levels, supports) = get_model_reasoning_config(name);
            assert!(supports, "{name}");
            assert_eq!(default, "high", "{name}");
            assert_eq!(levels.as_array().unwrap().len(), 6, "{name}");
        }
```

删除 `unknown_third_party_models_get_the_safe_three_level_ladder`（`:1296-1312`）、
`deepseek_v4_models_offer_the_extended_effort_ladder`（`:1313-1334`）与
`non_reasoning_models_offer_none_rather_than_nothing`（`:1336-1349`）三个用例——它们断言的旧语义
（三档 / 五档 / 只有 none）已被上面的用例整体覆盖。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-core every_model_gets_the_same_full_six_level_ladder`
Expected: FAIL — `qwen3.8-max` 只有三档，第一处断言即失败。

- [ ] **Step 3: 重写 `get_model_reasoning_config`**

在 `crates/core/src/profile_switch/codex_catalog.rs`，把 `:11-256` 的
`fallback_reasoning_levels()` 与 `get_model_reasoning_config()` 整体替换为：

```rust
/// The effort ladder every model is offered.
///
/// Codex decides the submenu from this list, not from the upstream, so the
/// catalogue must advertise every level the user may want to send. Whether the
/// upstream accepts a level is its own call: the gateway forwards what the user
/// picked and lets a 400 surface rather than silently narrowing the menu.
/// `none` stays selectable, so "no reasoning" remains reachable.
pub(super) fn full_reasoning_levels() -> serde_json::Value {
    serde_json::json!([
        { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
        { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
        { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
        { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
        { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
        { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
    ])
}

/// `(default, levels, supports_reasoning_summaries)` for a model.
///
/// One ladder for every model. Naming rules used to narrow it by family, which
/// meant any relay model whose name no rule recognised lost `xhigh`/`max` from
/// the menu entirely.
pub(super) fn get_model_reasoning_config(
    _slug: &str,
) -> (serde_json::Value, serde_json::Value, bool) {
    (serde_json::json!("high"), full_reasoning_levels(), true)
}
```

`build_codex_catalog_with_1m`（`:314`）的调用点与 `mod.rs:34` 的 `use` 都无需改动——函数名和签名都没变。

**Why:** 这正是 spec §2 的全部内容。家族表（sol/terra/luna/claude/gpt-5/gemini/o1 等七条分支）与 `fallback_reasoning_levels()` 全部消失。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-core --lib`
Expected: 全部 PASS。若 `every_model_advertises_at_least_one_reasoning_level`（`:1275`）失败，说明默认档不在表内——六档表里有 `high`，不应失败。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/profile_switch/codex_catalog.rs crates/core/src/profile_switch/mod.rs
git commit -m "feat(codex): 模型目录统一给六档思考深度，不再按家族收窄"
```

---

### Task 2: Messages 桥接原样透传深档

**Files:**
- Modify: `crates/core/src/messages_chat.rs:340-408`
- Test: `crates/core/src/messages_chat.rs:981-1000`（既有用例）

**Interfaces:**
- Consumes: `supports_extended_reasoning_effort`（本任务后不再被本文件使用，但该函数仍被 `responses_chat.rs` 使用，Task 3 才删）
- Produces: `normalize_message_effort(raw: &str, model: &str, warnings: &mut Vec<String>) -> Option<String>` — 签名保留，`model` 参数变为未使用（改名 `_model`）。

- [ ] **Step 1: 改写既有测试**

把 `crates/core/src/messages_chat.rs` 的 `clamps_extended_effort_for_models_that_do_not_support_it`（`:981`）替换为：

```rust
#[test]
fn passes_extended_effort_through_without_clamping() {
    let request = json!({
        "model": "plain-model",
        "max_tokens": 1024,
        "reasoning_effort": "max",
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let converted = messages_to_chat(&request, None).unwrap();
    assert_eq!(
        converted.body["reasoning_effort"], "max",
        "深档必须原样透传，由上游决定接不接受"
    );
    assert!(
        !converted.warnings.iter().any(|w| w.contains("降级")),
        "透传路径不该产生降级警告"
    );
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-core passes_extended_effort_through_without_clamping`
Expected: FAIL — 实际发出的是 `"high"`。

- [ ] **Step 3: 改 `normalize_message_effort`**

把 `:340-360` 的函数体改为：`normalize_message_effort` 的 match 里，`"low" | "medium" | "high" | "xhigh" | "max"` 合为一臂直接返回 `clean`，删掉 `supports_extended_reasoning_effort` 条件与随后的降级臂。`"minimal" => "low"`、`"none"|"off"|"0"|"false" => None`、数字反推三臂原样保留。

删掉降级臂后 `model` 参数不再被使用，把它改名为 `_model`（`clippy -D warnings` 会因 unused variable 失败）。调用方 `effort_from_request` 传 `model` 的写法不用改。

再把 `effort_from_request` 的 `budget_tokens` 分支（`:389-405`）改为直接 `budget_to_effort(budget)`，删掉里面针对 `xhigh`/`max` 的降级闭包。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-core --lib`
Expected: 全部 PASS，含既有的 `maps_reasoning_effort_levels` 中 deepseek-v4 的两条断言（它们本就期望 max/xhigh 透传）。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/messages_chat.rs
git commit -m "feat(bridge): Messages 桥接原样透传 xhigh/max 档位"
```

---

### Task 3: Responses 桥接原样透传深档

**Files:**
- Modify: `crates/core/src/responses_chat.rs:421-476`
- Test: `crates/core/src/responses_chat.rs:1035-1048`、`:1382-1423`（既有用例）

**Interfaces:**
- Consumes: 无
- Produces: 无新接口。`responses_to_chat` 签名不变。本任务删掉本文件对 `supports_extended_reasoning_effort` 的最后引用。

- [ ] **Step 1: 改写既有测试**

把 `downgrades_extended_reasoning_effort_for_chat_backends`（`:1035`）替换为：

```rust
#[test]
fn passes_extended_reasoning_effort_through_to_chat_backends() {
    let request = json!({
        "model": "dynamic-model", "input": "x", "reasoning": { "effort": "xhigh" }
    });
    let converted = responses_to_chat(&request, None).unwrap();
    assert_eq!(converted.body["reasoning_effort"], "xhigh");
    assert!(!converted.warnings.iter().any(|w| w.contains("降级")));
}
```

在 `maps_reasoning_effort_dynamically_per_model`（`:1382`）里改这四处：

- `req_gen`（`gpt-5.6-sol` + `xhigh`）：期望值由 `"high"` 改为 `"xhigh"`，注释改为「generic chat backend 也原样透传」。
- `req_gemini`（`gemini-2.5-pro` + `minimal`）：保持 `"low"` 不变。
- 追加 Gemini 深档用例：`{"model":"gemini-2.5-pro","input":"x","reasoning":{"effort":"max"}}` → 期望 `"max"`（原先会降为 `"high"`）。
- `req_ds_old`（`deepseek-v3.2` + `max`）：期望值由 `"high"` 改为 `"max"`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-core passes_extended_reasoning_effort_through_to_chat_backends`
Expected: FAIL — 实际值仍是 `"high"`。

- [ ] **Step 3: 简化 `responses_to_chat` 的 effort 块**

把 `:421-476` 整块改为：只保留自动思考模型的丢弃分支与 `minimal → low` 的映射，其余一律原样写入。

```rust
    if let Some(effort) = request.pointer("/reasoning/effort").and_then(Value::as_str) {
        let model_lower = model.to_ascii_lowercase();
        let effort_clean = effort.trim().to_ascii_lowercase();
        // Auto-thinking models run their own reasoning and 400 on an effort
        // field, so it is dropped rather than forwarded. Every other level is
        // the user's choice and travels verbatim.
        if model_lower.contains("reasoner")
            || model_lower.contains("r1")
            || model_lower.contains("qwq")
        {
            warnings.push(format!(
                "模型 {model} 自动执行思考推理，已安全忽略 reasoning_effort 参数以避免上游报错"
            ));
        } else if effort_clean == "minimal" {
            body.insert("reasoning_effort".into(), Value::String("low".into()));
            warnings.push("Chat 后端不支持 minimal 推理档位，已自动平滑映射为 low".into());
        } else if matches!(effort_clean.as_str(), "low" | "medium" | "high" | "xhigh" | "max") {
            body.insert("reasoning_effort".into(), Value::String(effort_clean));
        } else if effort_clean != "none" {
            warnings.push(format!(
                "Chat 后端不支持 Responses reasoning effort={effort}，已省略该字段"
            ));
        }
    }
```

Gemini 的独立分支整段删除（它只做了 `minimal→low` 与 `xhigh|max→high`，前者已被通用臂覆盖）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-core --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/responses_chat.rs
git commit -m "feat(bridge): Responses 桥接原样透传 xhigh/max 档位"
```

---

### Task 4: 网关能力合成改为全档位 + 补缺

**Files:**
- Modify: `crates/gateway/src/router/models.rs:50-116`、`:140-242`
- Test: `crates/gateway/src/router/mod.rs:2086-2270`

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub(super) fn synthesize_capabilities(thinking_support: ThinkingSupport) -> Value`（**参数从两个减为一个**，`serves_max` 删除）
  - `synthesize_models_response(raw: Value, thinking_support: ThinkingSupport, claude_max_output_tokens: Option<u64>) -> Value`（签名不变）

- [ ] **Step 1: 改写既有测试**

在 `crates/gateway/src/router/mod.rs` 的 `mod tests` 内：

1. 删除 `max_effort_detection_spans_suffix_spellings`（`:2172`）与 `max_effort_not_inferred_from_product_names`（`:2193`）——被测函数不存在了。
2. 删除 `ids_to_data` 辅助函数（只被上面两个用例使用）。
3. 把 `max_effort_offered_when_upstream_advertises_a_max_model`（`:2156`）替换为：

```rust
#[test]
fn every_model_advertises_the_full_effort_ladder() {
    let raw = serde_json::json!({
        "data": [
            { "id": "claude-opus-5" },
            { "id": "glm-5.3-flash" },
            { "id": "qwen-max" }
        ]
    });
    let resp = synthesize_models_response(raw, ThinkingSupport::Signed, Some(262_144));
    for m in resp["data"].as_array().unwrap() {
        for level in ["low", "medium", "high", "xhigh", "max"] {
            assert_eq!(
                m["capabilities"]["effort"][level]["supported"], true,
                "{} 的 {level} 档必须报为 supported",
                m["id"]
            );
        }
    }
}
```

4. 替换 `synthesize_models_response_adds_max_to_deepseek_v4_existing_capabilities`（`:2230`）：

```rust
#[test]
fn existing_capabilities_get_missing_levels_filled_in() {
    let raw = serde_json::json!({
        "data": [{
            "id": "glm-5.3-flash",
            "capabilities": { "effort": { "supported": true, "high": { "supported": true } } }
        }]
    });
    let resp = synthesize_models_response(raw, ThinkingSupport::Unsigned, None);
    let effort = &resp["data"][0]["capabilities"]["effort"];
    assert_eq!(effort["xhigh"]["supported"], true, "缺失的档位要补齐");
    assert_eq!(effort["max"]["supported"], true, "缺失的档位要补齐");
    assert_eq!(effort["high"]["supported"], true, "上游已写的值保留");
}

#[test]
fn upstream_denied_effort_support_is_not_rewritten() {
    let raw = serde_json::json!({
        "data": [{
            "id": "glm-5.3-flash",
            "capabilities": { "effort": { "supported": false } }
        }]
    });
    let resp = synthesize_models_response(raw, ThinkingSupport::Signed, None);
    assert_eq!(
        resp["data"][0]["capabilities"]["effort"]["supported"], false,
        "上游明确否认 effort 时不得改写"
    );
}
```

5. `synthesize_models_response_preserves_existing_capabilities`（`:2222`）保持不变——它与新行为一致。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-gateway every_model_advertises_the_full_effort_ladder`
Expected: FAIL — `glm-5.3-flash` 的 `max` 是 `false`。

- [ ] **Step 3: 改 `synthesize_capabilities` 与 `synthesize_models_response`**

在 `crates/gateway/src/router/models.rs`：

- 删除 `EFFORT_SUFFIX_SEPARATORS`（`:51`）与 `upstream_serves_max_effort`（`:63-81`）。
- `synthesize_capabilities` 去掉 `serves_max: bool` 参数，`"max": {"supported": true}` 写死。
- `synthesize_models_response` 删掉 `serves_max` 与 `model_serves_max` 的计算，`synthesize_capabilities` 调用只传 `thinking_support`。
- `capabilities` 已存在且是对象时，改成「补缺不覆盖」：对 `low/medium/high/xhigh/max` 五个键，只在键不存在时插入 `{"supported": true}`；`effort.supported` 与 `thinking.supported` 的既有处理不变（`effort.supported` 不覆盖，`thinking.supported` 按探测结果覆盖）。
- 文件头注释与 `mod.rs:48` 的 `use` 里去掉 `upstream_serves_max_effort`。

**Why:** spec §3。补缺而不是替换，是为了 `upstream_denied_effort_support_is_not_rewritten` 这条用户决定。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-gateway --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 同步别名用例的档位断言**

在 `crates/gateway/src/router/mod.rs:2126-2137` 的 `synthesize_models_response_adds_claude_visible_aliases` 里，把这段：

```rust
            let id = m["id"].as_str().unwrap();
            // DeepSeek V4 exposes max on the single model id; other models still
            // require a matching `-max` variant before the level is offered.
            let expected = id.starts_with("deepseek-v4") || id.contains("/deepseek-v4");
            assert_eq!(
                m["capabilities"]["effort"]["max"]["supported"], expected,
                "{id}"
            );
```

改为：

```rust
            // Every model now offers the full ladder, alias or not.
            assert_eq!(m["capabilities"]["effort"]["max"]["supported"], true);
```

Run: `cargo test -p polydeck-gateway --lib`
Expected: 全部 PASS。

- [ ] **Step 6: 跑接线测试**

Run: `cargo test -p polydeck-gateway --test thinking_discovery_wire`
Expected: 4 个用例 PASS（该文件不需要改动——它测的是 thinking 开关，与档位无关）。

- [ ] **Step 7: 提交**

```bash
git add crates/gateway/src/router/models.rs crates/gateway/src/router/mod.rs
git commit -m "feat(gateway): 模型能力统一报全档位，已有 capabilities 只补缺"
```

---

### Task 5: 删除共享能力猜测函数

**Files:**
- Modify: `crates/core/src/types.rs:73-81`
- Test: 无新增（由全仓编译保证）

**Interfaces:**
- Consumes: Task 2、Task 3 已移除全部调用点
- Produces: 无

- [ ] **Step 1: 确认已无调用点**

Run: `grep -rn "supports_extended_reasoning_effort" --include=*.rs crates/ src-tauri/`
Expected: 只剩 `types.rs` 的定义本身（以及 `messages_chat.rs` / `responses_chat.rs` 里已在该任务中删除的 `use`，若仍有则一并清掉）。

- [ ] **Step 2: 删除函数**

删掉 `crates/core/src/types.rs:73-81` 的文档注释与 `pub fn supports_extended_reasoning_effort`。同时清理两个桥接文件顶部 `use crate::types::supports_extended_reasoning_effort;`。

- [ ] **Step 3: 全仓编译验证**

Run: `cargo check --workspace --all-targets`
Expected: 通过，无 unused import 警告。

- [ ] **Step 4: 提交**

```bash
git add crates/core/src/types.rs crates/core/src/messages_chat.rs crates/core/src/responses_chat.rs
git commit -m "refactor(core): 删除模型名能力猜测函数 supports_extended_reasoning_effort"
```

---

### Task 6: 桥接路径读取 `output_config.effort`

**Files:**
- Modify: `crates/core/src/messages_chat.rs:362-408`（`effort_from_request`）、`:481-500`（`messages_to_chat` 主体）
- Test: `crates/core/src/messages_chat.rs`（`mod tests` 内新增两个用例）

**Interfaces:**
- Consumes: `normalize_message_effort`（Task 2 已改）、`budget_to_effort`
- Produces: 无签名变化。`effort_from_request` 内部新增第三个读取来源 `output_config.effort`。

- [ ] **Step 1: 写两个失败测试**

在 `crates/core/src/messages_chat.rs` 的 `mod tests` 内新增：

```rust
#[test]
fn output_config_effort_reaches_the_chat_body() {
    // Claude Code never sends `reasoning_effort`; max/xhigh travel as
    // `output_config.effort`. Without this the level is silently lost on a
    // relay that the gateway bridges to Chat Completions.
    let request = json!({
        "model": "glm-5.3-flash",
        "max_tokens": 4096,
        "output_config": { "effort": "max" },
        "thinking": { "type": "adaptive" },
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let converted = messages_to_chat(&request, None).unwrap();
    assert_eq!(converted.body["reasoning_effort"], "max");
    assert!(
        converted.body.get("output_config").is_none(),
        "Chat Completions 不认识 output_config，必须剥掉"
    );
}

#[test]
fn output_config_does_not_beat_an_explicit_reasoning_effort() {
    let request = json!({
        "model": "glm-5.3-flash",
        "max_tokens": 4096,
        "reasoning_effort": "low",
        "output_config": { "effort": "max" },
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let converted = messages_to_chat(&request, None).unwrap();
    assert_eq!(converted.body["reasoning_effort"], "low");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-core output_config`
Expected: 第一个用例 FAIL（`reasoning_effort` 缺失，实际为 null）；第二个可能已通过。

- [ ] **Step 3: 实现**

在 `effort_from_request` 的读取链里插入第三来源。当前的 `.get("reasoning_effort").or_else(|| thinking.effort)` 之后追加：

```rust
        .or_else(|| {
            request
                .pointer("/output_config/effort")
                .and_then(Value::as_str)
        })
```

位置必须在 `budget_tokens` 反推之前、`fallback_effort` 之前，顺序即 spec §5 的优先级。

在 `messages_to_chat` 的 `body` 组装完成、`effort_from_request` 调用前后，把 `output_config` 从请求体剥掉——它本来就只在这里读，从不插入 `body`，因此只需确认没有任何分支把它复制进 `body`。若实现时发现它在别处被复制，在 `Ok(ConvertedMessagesRequest {...})` 之前加 `body.remove("output_config");` 兜底。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-core --lib`
Expected: 全部 PASS，含新增两个用例与 Task 2 的透传用例。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/messages_chat.rs
git commit -m "feat(bridge): 桥接路径读取 output_config.effort，max 不再丢失"
```

---

### Task 7: 写入强制能力 env

**Files:**
- Modify: `crates/core/src/profile_switch/mod.rs:716-733`（`CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY` 之后）
- Test: `crates/core/src/profile_switch/mod.rs`（`mod tests` 内新增一个用例）

**Interfaces:**
- Consumes: `write_claude_config(provider, params, auth, gateway_enabled)`（既有签名）
- Produces: 无新接口。env 键名常量值固定为 `*=effort,max_effort,xhigh_effort`。

- [ ] **Step 1: 写失败测试**

在 `crates/core/src/profile_switch/mod.rs` 的 `mod tests` 内新增：

```rust
#[tokio::test]
async fn claude_env_forces_full_effort_capabilities_behind_the_gateway() {
    let _home_guard = lock_home_env();
    let temp_home = tempfile::tempdir().unwrap();
    std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
    let claude_dir = temp_home.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    let provider = agnes_like_provider(vec!["glm-5.3-flash".into()], "glm-5.3-flash");

    // 网关开启：写入强制能力，覆盖 Claude Code 内置目录（haiku 档连 effort 都没有）
    write_claude_config(&provider, &Default::default(), "adk_test", true)
        .await
        .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(
        parsed["env"]["CLAUDE_CODE_MODEL_CAPABILITIES"].as_str(),
        Some("*=effort,max_effort,xhigh_effort")
    );

    // 网关关闭：残留值必须删除，否则会继续覆盖直连模式的真实能力
    write_claude_config(&provider, &Default::default(), "adk_test", false)
        .await
        .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert!(
        parsed["env"].get("CLAUDE_CODE_MODEL_CAPABILITIES").is_none(),
        "关闭网关后不得留下强制能力 env"
    );
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p polydeck-core claude_env_forces_full_effort_capabilities_behind_the_gateway`
Expected: FAIL — 该键不存在。

- [ ] **Step 3: 实现**

在 `write_claude_config` 里 `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY` 的那段 `env_obj.insert(...)` 之后追加：

```rust
        // Claude Code gates the effort picker on `effort` plus `max_effort` /
        // `xhigh_effort` in its built-in model catalogue, which takes
        // precedence over anything `/v1/models` reports. `claude-haiku-4-5`
        // ships with no `effort` at all, so that tier would show no picker
        // even though the gateway advertises every level. `*` is a prefix match
        // on the model name, so this covers every discovered id.
        //
        // Written on both branches: this file is merged, not replaced, and a
        // gateway-less profile inheriting the override would claim capabilities
        // the direct upstream may not have.
        if gateway_enabled {
            env_obj.insert(
                "CLAUDE_CODE_MODEL_CAPABILITIES".into(),
                serde_json::Value::String("*=effort,max_effort,xhigh_effort".into()),
            );
        } else {
            env_obj.remove("CLAUDE_CODE_MODEL_CAPABILITIES");
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p polydeck-core --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/profile_switch/mod.rs
git commit -m "feat(claude): 网关模式下强制开放全模型思考档位能力"
```

---

### Task 8: 更新文档

**Files:**
- Modify: `README.md:38-43`
- Modify: `CHANGELOG.md:3-5`

**Interfaces:**
- Consumes: 无
- Produces: 无

- [ ] **Step 1: 改 README 的档位描述**

把 `README.md` 第 38-43 行的 Sol / Terra / Luna 三条家族档位说明（"Sol 支持全档位"、"Terra 动态自适应最高至 xhigh"、"Luna 最高自适应至 high"）替换为一段：

```markdown
- **全模型统一思考档位**：`none / low / medium / high / xhigh / max` 六档对所有模型开放，
  不再按模型家族收窄。档位原样透传给上游，由上游决定是否接受——不支持的模型会返回上游
  自身的错误，网关不做静默降级。
```

- [ ] **Step 2: 改 CHANGELOG**

在 `CHANGELOG.md` 的 `## [Unreleased]` 下加：

```markdown
### Changed
- **所有模型的思考档位无条件开放。** Codex 模型目录、网关 `/v1/models` 能力声明都改为
  六档（`none / low / medium / high / xhigh / max`），Messages 与 Responses 两条桥接
  路径不再按模型家族把 `xhigh` / `max` 降级成 `high`。上游明确声明不支持 effort 的
  模型保持原样，不做改写

### Fixed
- **走 Chat 桥接时 Claude Code 的 `max` 档位丢失。** Claude Code 以
  `output_config.effort` 发送档位，桥接路径此前只读 `reasoning_effort`，现在一并读取
  并把 `output_config` 从 Chat 转发体中剥掉
- **Claude Code 的 haiku 档位槽没有档位选择器。** 内置目录 `claude-haiku-4-5` 不含
  `effort` 能力，网关模式下写入 `CLAUDE_CODE_MODEL_CAPABILITIES` 覆盖
```

- [ ] **Step 3: 提交**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: 更新全模型思考档位的说明"
```

---

### 最终验证

- [ ] **跑六道门禁**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run lint && npx tsc --noEmit
npm test
```

Expected: 全部通过，零警告。

- [ ] **确认无残留引用**

Run: `grep -rn "supports_extended_reasoning_effort\|upstream_serves_max_effort\|fallback_reasoning_levels" --include=*.rs crates/ src-tauri/`
Expected: 无输出。

- [ ] **确认行为**

启动网关后请求 `GET http://127.0.0.1:18888/v1/models`，检查每个条目的
`capabilities.effort.{low,medium,high,xhigh,max}.supported` 均为 `true`（上游显式否认的除外）。

【无法确认】真实 Codex CLI / Claude Code 客户端与真实中转站的端到端行为——需要装包实测。




