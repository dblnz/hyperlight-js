function fibonacci(n) {
  if (n <= 0) return 0;
  if (n === 1) return 1;
  return fibonacci(n - 1) + fibonacci(n - 2);
}

function factorial(n) {
  if (n <= 1) return 1;
  return n * factorial(n - 1);
}

function isPrime(n) {
  if (n < 2) return false;
  for (let i = 2; i <= Math.sqrt(n); i++) {
    if (n % i === 0) return false;
  }
  return true;
}

function collectPrimes(limit) {
  const primes = [];
  for (let i = 2; i <= limit; i++) {
    if (isPrime(i)) {
      primes.push(i);
    }
  }
  return primes;
}

function sumArray(arr) {
  let total = 0;
  for (let i = 0; i < arr.length; i++) {
    total += arr[i];
  }
  return total;
}

function buildFibonacciSequence(count) {
  const sequence = [];
  for (let i = 0; i < count; i++) {
    sequence.push(fibonacci(i));
  }
  return sequence;
}

console.log("Loading fibonacci handler\n");

// --- Run some logic at load time for debugging ---

// Verify fibonacci works with a small value
const testFib = fibonacci(8);
console.log("Load-time check: fibonacci(8) =", testFib, "\n");

// Compute a few factorials at load time
for (let k = 1; k <= 6; k++) {
  console.log("Load-time check: factorial(" + k + ") =", factorial(k), "\n");
}

// Find primes up to 30 and display them
const loadPrimes = collectPrimes(30);
console.log("Load-time check: primes up to 30:", JSON.stringify(loadPrimes), "\n");

// Build a short fibonacci sequence and sum it
const loadSeq = buildFibonacciSequence(10);
const loadSum = sumArray(loadSeq);
console.log("Load-time check: first 10 fibonacci numbers:", JSON.stringify(loadSeq), "\n");
console.log("Load-time check: sum of first 10 fibonacci numbers:", loadSum, "\n");

console.log("Fibonacci handler loaded successfully\n");

// --- End load-time logic ---

function handler({ n }) {
  console.log("Computing fibonacci for n =", n, "\n");

  // Build the full fibonacci sequence up to n
  const fibSequence = buildFibonacciSequence(n + 1);
  console.log("Fibonacci sequence:", JSON.stringify(fibSequence), "\n");

  // Compute factorial of n (capped at 12 to avoid overflow)
  const factN = factorial(Math.min(n, 12));
  console.log("Factorial of", Math.min(n, 12), "=", factN, "\n");

  // Find all primes up to the fibonacci result
  const fibResult = fibSequence[n];
  const primes = collectPrimes(fibResult);
  console.log("Primes up to", fibResult, ":", JSON.stringify(primes), "\n");

  // Sum up the fibonacci sequence
  const fibSum = sumArray(fibSequence);
  console.log("Sum of fibonacci sequence:", fibSum, "\n");

  // Sum of primes
  const primeSum = sumArray(primes);
  console.log("Sum of primes:", primeSum, "\n");

  // Build a summary object
  const result = {
    input: n,
    fibonacci: fibResult,
    factorial: factN,
    fibSequence: fibSequence,
    primes: primes,
    fibSum: fibSum,
    primeSum: primeSum,
  };

  console.log("Result:", JSON.stringify(result), "\n");

  return result;
}

export { handler };
