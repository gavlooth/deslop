/* @generated */
export function compute(π, items) {
  // line comment
  let total = π * 2;
  with (Math) {
    total += max(...items);
  }
  return total > 10 ? total : 0;
}
