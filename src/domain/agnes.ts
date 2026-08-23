/**
 * Agnes AI provider constants.
 *
 * Mirrors `crates/core/src/profile_templates.rs`. The backend template is the
 * source of truth for a saved profile; these exist so the Quick Setup panel and
 * the Profiles dropdown can offer Agnes before any backend call happens. Keep
 * the two in step — `src/test/agnes.test.ts` pins the values that were measured
 * against the live endpoint.
 */

/** Mainland China route. Keys issued at platform.agnes-ai.cn work here. */
export const AGNES_BASE_URL_CN = "https://api.agnes-ai.cn/v1";

/**
 * International route. Same model catalogue, but a **separate key scope**.
 *
 * Measured: a CN-issued key gets 200 from this host's `/v1/models` yet 401
 * (`无效的令牌`) on `/v1/chat/completions`. So listing models is not proof the
 * route will serve inference — the site has to match where the key was created.
 * The provider probe does catch this, because it validates with a chat call
 * rather than trusting the model list.
 */
export const AGNES_BASE_URL_GLOBAL = "https://apihub.agnes-ai.com/v1";

/**
 * Free tier, and the stronger of the two free models for coding and agent work.
 * `agnes-2.0-flash` is the one Agnes's own integration guides still name, but
 * 2.5 supersedes it on the same free pricing.
 */
export const AGNES_DEFAULT_MODEL = "agnes-2.5-flash";

/**
 * Measured ceiling on a free key: the 17th request inside a minute returns 429.
 * Agnes sends no `RateLimit-*` response headers, so the provider probe cannot
 * discover this and leaves its generic 60 in place — three times over the real
 * limit. Pinned deliberately.
 */
export const AGNES_FREE_TIER_RPM = 20;

/** Where an Agnes API key is created. */
export const AGNES_CONSOLE_URL = "https://platform.agnes-ai.cn/";

export interface AgnesRoute {
  id: "agnes-cn" | "agnes-global";
  label: string;
  baseUrl: string;
  hint: string;
}

/**
 * The two routes. Key scope is per-site, so this is not merely a network
 * preference — it has to match the site the key was issued on.
 */
export const AGNES_ROUTES: AgnesRoute[] = [
  {
    id: "agnes-cn",
    label: "国内站",
    baseUrl: AGNES_BASE_URL_CN,
    hint: "国内网络直连 · 配 platform.agnes-ai.cn 的 Key",
  },
  {
    id: "agnes-global",
    label: "国际站",
    baseUrl: AGNES_BASE_URL_GLOBAL,
    hint: "海外网络优选 · 需国际站账号的 Key",
  },
];

/**
 * Shown under the route picker. A key from the other site lists models fine and
 * then 401s on the first real request, so the mismatch is worth naming up front.
 */
export const AGNES_ROUTE_KEY_SCOPE_NOTE =
  "两站的 API Key 不通用。请选择与你的 Key 所属站点一致的线路，否则「获取模型」能成功、真实对话会返回 401。";

export interface AgnesModelChoice {
  id: string;
  label: string;
  note: string;
  free: boolean;
}

/**
 * Text models only. `/v1/models` also returns `agnes-image-2.1-flash`,
 * `agnes-video-2.5` and `agnes-video-v2.0`, but those answer on
 * `/v1/images/generations` and `/v1/videos` — offering them as a chat model
 * would hand the user a selection that fails.
 */
export const AGNES_MODELS: AgnesModelChoice[] = [
  {
    id: "agnes-2.5-flash",
    label: "Agnes 2.5 Flash",
    note: "512K 上下文 · 编码与 Agent 优化 · 推荐",
    free: true,
  },
  {
    id: "agnes-2.0-flash",
    label: "Agnes 2.0 Flash",
    note: "512K 上下文 · 官方集成指南沿用的版本",
    free: true,
  },
  {
    id: "agnes-2.5-pro",
    label: "Agnes 2.5 Pro",
    note: "1M 上下文 · 推理模型 · 按量计费",
    free: false,
  },
  {
    id: "agnes-2.5-pro-alpha",
    label: "Agnes 2.5 Pro Alpha",
    note: "1M 上下文 · 推理模型 · 按量计费",
    free: false,
  },
];

/** Every Agnes text model id, in the order the picker shows them. */
export const AGNES_MODEL_IDS = AGNES_MODELS.map((m) => m.id);

/**
 * Reasoning models bill thinking against the output budget, so a small
 * `max_tokens` can return reasoning and no answer. Surfaced as a UI warning
 * when a Pro model is chosen.
 */
export const AGNES_PRO_BUDGET_WARNING =
  "Pro 系列会先消耗输出预算做推理，max_tokens 给小了可能只返回思考、没有正文。建议留足预算，日常编码优先用 2.5 Flash。";
