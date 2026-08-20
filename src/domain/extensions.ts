export interface McpServer {
  id: string;
  name: string;
  description: string;
  command: string;
  args: string[];
  envKeys: string[];
  isBuiltin: boolean;
}

export type SkillSource =
  | "builtin"
  | { gitHub: { repo: string; path: string | null } };

export interface ManagedSkill {
  id: string;
  name: string;
  description: string;
  source: SkillSource;
  enabled: boolean;
}

export type PromptScope = "global" | "profile";

export interface PromptTemplate {
  id: string;
  name: string;
  content: string;
  variables: string[];
  scope: PromptScope;
}