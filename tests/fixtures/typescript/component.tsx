import type { HTMLAttributes } from "react";

type Item = { id: string; label: string };

interface Props<T extends Item> extends HTMLAttributes<HTMLUListElement> {
  items: readonly T[];
}

const identity = <T,>(value: T): T => value;

export function View<T extends Item>({ items, ...rest }: Props<T>): JSX.Element {
  return (
    <>
      <UI.List<string> {...rest}>
        {items.map((item) => (
          <UI.Item key={item.id}>{identity(item.label)}</UI.Item>
        ))}
      </UI.List>
    </>
  );
}
