import type { HTMLAttributes } from "react";

interface Props extends HTMLAttributes<HTMLDivElement> {
  title: string;
}

export function View({ title, ...rest }: Props): JSX.Element {
  // JSX remains surface syntax, not type authority.
  return (
    <UI.Panel {...rest}>
      <span>{title}</span>
    </UI.Panel>
  );
}
