export type PlaygroundExample = {
  id: string;
  label: string;
  filename: string;
  code: string;
  expected: string;
};

export const MAX_SHARE_URL_LENGTH = 8000;

const HEREDOC_DELIMITER = "TURBO_PLAYGROUND_EOF";

export const examples: PlaygroundExample[] = [
  {
    id: "hello",
    label: "Hello World",
    filename: "hello.tb",
    code: `fn main() {
    print("Hello, browser!")
}`,
    expected: "Hello, browser!",
  },
  {
    id: "fizzbuzz",
    label: "FizzBuzz",
    filename: "fizzbuzz.tb",
    code: `fn main() {
    let mut i = 1
    while i <= 20 {
        if i % 15 == 0 {
            print("FizzBuzz")
        } else if i % 3 == 0 {
            print("Fizz")
        } else if i % 5 == 0 {
            print("Buzz")
        } else {
            print(i)
        }
        i += 1
    }
}`,
    expected: "1\n2\nFizz\n4\nBuzz\n...\n19\nBuzz",
  },
  {
    id: "matching",
    label: "Pattern Matching",
    filename: "shape.tb",
    code: `type Shape {
    Circle(f64)
    Rectangle(f64, f64)
}

fn area(shape: Shape) -> f64 {
    match shape {
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
    }
}

fn main() {
    let s = Shape.Circle(5.0)
    print("Area: {area(s)}")
}`,
    expected: "Area: 78.53975",
  },
  {
    id: "collections",
    label: "Collections",
    filename: "scores.tb",
    code: `fn main() {
    let scores = [3, 5, 8, 13]
    let mut total = 0

    for score in scores {
        total += score
    }

    print("Total: {total}")
    print("Count: {len(scores)}")
}`,
    expected: "Total: 29\nCount: 4",
  },
];

export const defaultExample = examples[0];

export function lineNumbersFor(code: string): number[] {
  return Array.from(
    { length: Math.max(1, code.split("\n").length) },
    (_, i) => i + 1
  );
}

export function commandFor(example: PlaygroundExample, code: string): string {
  const delimiter = heredocDelimiterFor(code);
  return `cat > ${example.filename} <<'${delimiter}'\n${code}\n${delimiter}\nturbolang run ${example.filename}`;
}

export function shareUrlFor(currentHref: string, code: string): string {
  const url = new URL(currentHref);
  url.pathname = "/play";
  url.search = "";
  url.hash = "";
  url.searchParams.set("code", code);
  return url.toString();
}

function heredocDelimiterFor(code: string): string {
  const sourceLines = new Set(code.split(/\r?\n/));
  let delimiter = HEREDOC_DELIMITER;
  let suffix = 1;

  while (sourceLines.has(delimiter)) {
    delimiter = `${HEREDOC_DELIMITER}_${suffix}`;
    suffix += 1;
  }

  return delimiter;
}
