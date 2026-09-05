export interface SessionSummary {
  id: string;
  client: string;
  title: string;
  messageCount: number;
  totalTokens: number;
  createdAt: string;
  updatedAt: string;
  /** Provider this conversation ran against, when recorded. */
  providerId: string | null;
  /** Profile bound to the client when the conversation was indexed. */
  profileId: string | null;
  /** Above 1 when duplicate rows for one conversation were folded together. */
  mergedFrom: number;
}

/** What a consolidation pass changed. */
export interface ConsolidateReport {
  clientsNormalized: number;
  timestampsNormalized: number;
  identitiesFilled: number;
  duplicatesMerged: number;
  sessionsAfter: number;
}

export interface UsageStats {
  totalSessions: number;
  totalMessages: number;
  totalTokens: number;
  sessionsByClient: Record<string, number>;
  sessionsByDate: [string, number][];
}