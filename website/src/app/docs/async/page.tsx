import Link from "next/link";

export default function AsyncPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">
        Concurrency
      </h1>
      <p className="text-lg text-gray-400 mb-8">
        Thread-based concurrency with spawn/await, channels, and
        synchronization primitives.
      </p>

      <div className="bg-yellow-900/20 border border-yellow-700/50 rounded-lg p-4 mb-8">
        <p className="text-yellow-300 font-medium mb-2">How concurrency works in Turbo</p>
        <p className="text-gray-300 mb-0 text-sm">
          Turbo uses <strong className="text-white">OS threads</strong> for concurrency.{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">spawn</code>{" "}
          creates a new OS thread (via pthread_create), and{" "}
          <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">await</code>{" "}
          joins it (via pthread_join). There is no event loop, no green threads, and no non-blocking I/O runtime.
          Each spawned task is a real OS thread, so avoid spawning thousands of tasks -- each one
          consumes real system resources.
        </p>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Async Functions
      </h2>
      <p className="mb-4">
        Declare functions with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          async fn
        </code>
        {" "}to mark them as spawnable:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`async fn compute(x: i64) -> i64 {
    x * x + 1
}

fn main() {
    let handle = spawn compute(5)
    let result = await handle
    print(result)    // 26
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Spawn and Await
      </h2>
      <p className="mb-4">
        Use{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          spawn
        </code>{" "}
        to run a function on a new OS thread and{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          await
        </code>{" "}
        to block until it completes and get its result:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`async fn fetch_data() -> i64 {
    sleep(100)
    42
}

fn main() {
    let handle = spawn fetch_data()
    // ... do other work while fetch_data runs on another thread ...
    let result = await handle    // blocks until the thread finishes
    print(result)    // 42
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Channels</h2>
      <p className="mb-4">
        Channels provide message passing between concurrent tasks:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn producer(ch: i64) {
    for i in 0..5 {
        send(ch, i * 10)
    }
}

fn main() {
    let ch = channel()
    spawn producer(ch)

    let mut i = 0
    while i < 5 {
        let val = recv(ch)
        print(val)          // 0, 10, 20, 30, 40
        i += 1
    }
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Mutex</h2>
      <p className="mb-4">
        Protect shared state with mutex primitives:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn main() {
    let m = mutex(0)

    // Read the value
    let val = mutex_get(m)
    print(val)          // 0

    // Update the value
    mutex_set(m, 42)
    print(mutex_get(m)) // 42
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">Sleep</h2>
      <p className="mb-4">
        Pause execution for a given number of milliseconds:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`async fn delayed_greeting() -> str {
    sleep(1000)    // wait 1 second
    "Hello after delay!"
}

fn main() {
    let handle = spawn delayed_greeting()
    print("Waiting...")
    let msg = await handle
    print(msg)
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Concurrent Patterns
      </h2>
      <p className="mb-4">
        Spawn multiple threads and collect results. Keep the thread count
        reasonable -- each spawn creates a real OS thread:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`async fn compute_square(x: i64) -> i64 {
    x * x
}

fn main() {
    // Spawn multiple concurrent tasks
    let h1 = spawn compute_square(10)
    let h2 = spawn compute_square(20)
    let h3 = spawn compute_square(30)

    // Await all results
    let a = await h1
    let b = await h2
    let c = await h3
    print(a + b + c)    // 100 + 400 + 900 = 1400
}`}</code>
      </pre>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/agents"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Agents
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
