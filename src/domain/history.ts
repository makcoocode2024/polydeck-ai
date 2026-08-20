export interface SessionSummary {
  id: string;
  client: string;
  title: string;
  messageCount: number;
  totalTokens: number;
  createdAt: string;
  updatedAt: string;
}

export interface UsageStats {
  totalSessions: number;
  totalMessages: number;
  totalTokens: number;
  sessionsByClient: Record<string, number>;
  sessionsByDate: [string, number][];
}