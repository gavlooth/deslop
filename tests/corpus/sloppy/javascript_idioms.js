var count = 0;

async function load(value) {
  if (value == null) {
    count = count + 1;
  }
  return await fetch("/items");
}
