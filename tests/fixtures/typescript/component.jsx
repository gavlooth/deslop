export function JsxView({ title, ...rest }) {
  return (
    <>
      <UI.Panel {...rest}>
        <UI.Icon name="marker" />
        <span>{title}</span>
      </UI.Panel>
    </>
  );
}
