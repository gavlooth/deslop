import type { Config } from "./config";

export interface Entity {
  id: string;
}

export type Result<T> =
  | { ok: true; value: T }
  | { ok: false; error: Error };

export function convert(value: string): number;
export function convert(value: number): string;
export function convert(value: string | number): number | string {
  return typeof value === "string" ? value.length : String(value);
}

@sealed
export class Repository<T extends Entity> {
  #items: T[] = [];

  add(item: T): this {
    this.#items.push(item);
    return this;
  }

  find(id: string): T | undefined {
    return this.#items.find((item): item is T => item.id === id);
  }
}

export const defaults = { limit: 10 } satisfies Config;

export namespace Internal {
  export const marker: unique symbol = Symbol();
}

export type { Config };
