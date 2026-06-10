export function env(name: string): string | undefined;
export function env(name: string, defaultValue: string): string;
export function env(
  name: string,
  defaultValue?: string,
): string | undefined {
  return process.env[name] ?? defaultValue;
}
