export interface IconRef {
  kind: string;
  data: string;
}

export function isIconRef(value: unknown): value is IconRef {
  return (
    typeof value === 'object' &&
    value !== null &&
    'kind' in value &&
    'data' in value
  );
}

export interface Match {
  id: string;
  provider: string;
  title: string;
  subtitle?: string;
  icon?: string | IconRef;
  score: number;
  action: {
    kind: string;
    data: unknown;
  };
}
