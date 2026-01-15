function fibonacci(n) {
  if (n <= 0) return 0;
  if (n === 1) return 1;
  return fibonacci(n - 1) + fibonacci(n - 2);
}

console.log("Loading simple handler\n");
console.log("Test 2 \n");

function test2() {
  print("This is test2 function\n");
  console.log("This", "is", "test2", "function", "\n");
}

function test() {
  print("This is a test function\n");
  console.log("This", "is", "a", "test", "function", "\n");
}

test();

function handler({ n }) {
  let number = 0;
  for (let i = 0; i < 10; i++) {
    number += i;
  }
  test();
  print("Hello World " + number + "\n");
  test2();
  console.log("Hello", "from", "Hyperlight JS!", "\n");
  return fibonacci(n);
}

export { handler };
