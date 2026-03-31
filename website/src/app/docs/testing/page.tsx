import Link from "next/link";

export default function TestingPage() {
  return (
    <article className="text-gray-300 leading-relaxed font-[family-name:var(--font-geist-sans)]">
      <h1 className="text-4xl font-bold text-white mb-4">Testing</h1>
      <p className="text-lg text-gray-400 mb-8">
        Turbo has a built-in test framework. Write tests alongside your code and
        run them with a single command.
      </p>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        @test Functions
      </h2>
      <p className="mb-4">
        Mark any function with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          @test
        </code>{" "}
        to make it a test case. Test functions must have no parameters and no
        return type:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn add(a: i64, b: i64) -> i64 { a + b }

@test fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(-1, 1), 0)
    assert_eq(add(0, 0), 0)
}

@test fn test_negative() {
    assert_eq(add(-5, -3), -8)
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Assertion Functions
      </h2>
      <div className="overflow-x-auto mb-8">
        <table className="w-full text-sm text-left border border-[#1a1a2e] rounded-lg overflow-hidden">
          <thead className="bg-[#111118] text-gray-400">
            <tr>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Function</th>
              <th className="px-4 py-2 border-b border-[#1a1a2e]">Description</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["assert(condition)", "Fails if condition is false"],
              ["assert_eq(a, b)", "Fails if a != b"],
              ["assert_ne(a, b)", "Fails if a == b"],
            ].map(([fn_, desc]) => (
              <tr key={fn_} className="border-b border-[#1a1a2e]">
                <td className="px-4 py-2">
                  <code className="text-[#00ff88] font-[family-name:var(--font-geist-mono)] text-xs">{fn_}</code>
                </td>
                <td className="px-4 py-2">{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Running Tests
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`$ turbo test myfile.tb
  PASS  test_add
  PASS  test_negative
2 passed, 0 failed`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Example: Testing with Assertions
      </h2>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {
    assert(add(2, 3) == 5)
    assert(10 > 5)
    assert(true)
    print("All assertions passed!")
}`}</code>
      </pre>

      <h2 className="text-2xl font-bold text-white mt-10 mb-4">
        Integration Tests
      </h2>
      <p className="mb-4">
        Turbo also supports integration tests using{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          .tb
        </code>{" "}
        +{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          .expected
        </code>{" "}
        file pairs:
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# File: my_test.tb
fn main() {
    print("hello")
    print("world")
}

# File: my_test.expected
hello
world`}</code>
      </pre>
      <p className="mb-4">
        The test runner compiles and runs the{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          .tb
        </code>{" "}
        file, captures stdout, and diffs it against the{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          .expected
        </code>{" "}
        file.
      </p>

      <h3 className="text-xl font-bold text-white mt-8 mb-4">
        Expected-Error Tests
      </h3>
      <p className="mb-4">
        To test that the compiler correctly rejects invalid code, prefix the
        expected file with{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          ERROR:
        </code>
        :
      </p>
      <pre className="bg-[#111118] border border-[#1a1a2e] rounded-lg p-4 mb-6 overflow-x-auto text-sm font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{`# File: type_error.expected
ERROR:type mismatch`}</code>
      </pre>
      <p className="mb-6">
        The test passes if the compiler error output contains the pattern after{" "}
        <code className="text-[#00ff88] bg-[#111118] px-1.5 py-0.5 rounded text-sm font-[family-name:var(--font-geist-mono)]">
          ERROR:
        </code>
        .
      </p>

      <div className="flex gap-4 mt-10">
        <Link
          href="/docs/error-codes"
          className="inline-flex items-center gap-2 bg-[#00ff88] text-[#0a0a0a] font-semibold px-6 py-3 rounded-lg hover:bg-[#00cc6a] transition-colors"
        >
          Next: Error Codes
          <span>&#8594;</span>
        </Link>
      </div>
    </article>
  );
}
