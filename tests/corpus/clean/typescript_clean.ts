let count = 0;

async function load(value: string | null) {
  if (value === null) {
    count = count + 1;
  }
  return fetch("/items");
}
