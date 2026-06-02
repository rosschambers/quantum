export interface Match {
  id: string;
  provider: string;
  title: string;
  subtitle?: string;
  icon?: string;
  score: number;
  action: {
    kind: string;
    data: unknown;
  };
}
