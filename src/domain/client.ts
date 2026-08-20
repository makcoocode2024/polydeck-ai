export interface DetectedClient {
  id: string;
  name: string;
  installed: boolean;
  version: string | null;
  configPath: string | null;
  supportsAutoConfig: boolean;
}