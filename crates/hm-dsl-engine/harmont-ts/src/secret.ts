const SECRET_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const SECRET_BRAND = Symbol("harmont.secretRef");

/** A reference to a stored secret, identified by name. Never holds a value. */
export interface SecretRef {
  readonly [SECRET_BRAND]: true;
  readonly name: string;
}

export function isSecretRef(v: unknown): v is SecretRef {
  return typeof v === "object" && v !== null && (v as Record<symbol, unknown>)[SECRET_BRAND] === true;
}

/**
 * `secrets["NAME"]` returns a SecretRef — a reference resolved at run time
 * (locally from .env + the process env; in the cloud from the secret store).
 */
export const secrets: Readonly<Record<string, SecretRef>> = new Proxy(
  {},
  {
    get(target, prop: string | symbol): SecretRef | undefined {
      // Only string subscripting names a secret; symbol access defers to the
      // target (so the object is not an accidental thenable, etc.).
      if (typeof prop === "symbol") {
        return (target as Record<symbol, never>)[prop];
      }
      const name = prop;
      if (!SECRET_NAME.test(name)) {
        throw new Error(
          `secret name "${name}" is invalid: names must match [A-Za-z_][A-Za-z0-9_]* ` +
            `(letters, digits, underscores; no leading digit).`,
        );
      }
      return { [SECRET_BRAND]: true, name };
    },
  },
) as Readonly<Record<string, SecretRef>>;
