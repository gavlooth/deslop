/* @generated */
interface Item {
  id: string;
  value: number;
}

@generated
export class Repository<T extends Item> {
  #items: T[] = [];

  add(item: T): this {
    this.#items.push(item);
    return this;
  }

  find(id: string): T | undefined {
    return this.#items.find((item): item is T => item.id === id);
  }
}
