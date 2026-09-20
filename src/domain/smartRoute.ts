// Hand-written mirror of the Rust types in crates/core/src/smart_route.rs and
// crates/gateway/src/router/smart_route.rs. camelCase on the wire, matching
// the serde attributes there.

export interface CustomRouteRule {
  id: string;
  keyword: string;
  targetModel: string;
  isRegex: boolean;
  enable: boolean;
}

export interface CategoryRouteMap {
  complexCore: string | null;
  regularDev: string | null;
  visualFrontend: string | null;
}

export interface SmartRouteSettings {
  enableRoute: boolean;
  forceModel: string | null;
  defaultModel: string | null;
  fallbackEnable: boolean;
  modelAliasMap: Record<string, string>;
  categoryRouteMap: CategoryRouteMap;
  customRules: CustomRouteRule[];
}

export type RouteReason =
  | "force_model"
  | "model_tag"
  | "custom_rule"
  | { category: string }
  | "default_model"
  | "passthrough"
  | "validation_fallback"
  | "fallback_retry";

export interface RouteDecision {
  routedModel: string;
  reason: RouteReason;
  matchRule: string | null;
  isTagHit: boolean;
  validationWarn: string | null;
}

export interface RouteAuditRecord {
  requestId: string;
  timestamp: string;
  clientId: string;
  endpoint: string;
  originalModel: string;
  aliasedModel: string;
  routedModel: string;
  routeReason: string;
  matchRule: string | null;
  thinkingEffort: string | null;
  inputTokens: number;
  isFallback: boolean;
}

export const CATEGORY_LABELS: Record<keyof CategoryRouteMap, string> = {
  complexCore: "复杂核心（架构/权限/财务/排课，effort=max，>64k）",
  regularDev: "常规开发（CRUD/迁移/测试/文档，兜底分类）",
  visualFrontend: "视觉前端（UI 组件/截图改页面/多模态）",
};

export const defaultSmartRouteSettings = (): SmartRouteSettings => ({
  enableRoute: false,
  forceModel: null,
  defaultModel: null,
  fallbackEnable: false,
  modelAliasMap: {},
  categoryRouteMap: { complexCore: null, regularDev: null, visualFrontend: null },
  customRules: [],
});
