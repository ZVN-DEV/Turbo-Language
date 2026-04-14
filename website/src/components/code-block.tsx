import React from "react";

function highlightTurbo(code: string): React.ReactNode[] {
  const lines = code.split("\n");
  return lines.map((line, lineIdx) => {
    const tokens: React.ReactNode[] = [];
    let remaining = line;
    let key = 0;

    while (remaining.length > 0) {
      // Comments
      const commentMatch = remaining.match(/^(\/\/.*)/);
      if (commentMatch) {
        tokens.push(
          <span key={key++} className="text-gray-500">
            {commentMatch[1]}
          </span>
        );
        remaining = remaining.slice(commentMatch[1].length);
        continue;
      }

      // Strings
      const stringMatch = remaining.match(/^("(?:[^"\\]|\\.)*")/);
      if (stringMatch) {
        tokens.push(
          <span key={key++} className="text-cyan-400">
            {stringMatch[1]}
          </span>
        );
        remaining = remaining.slice(stringMatch[1].length);
        continue;
      }

      // Numbers
      const numMatch = remaining.match(/^(\b\d+(?:\.\d+)?\b)/);
      if (numMatch) {
        tokens.push(
          <span key={key++} className="text-orange-400">
            {numMatch[1]}
          </span>
        );
        remaining = remaining.slice(numMatch[1].length);
        continue;
      }

      // Keywords
      const kwMatch = remaining.match(
        /^(\b(?:fn|let|mut|if|else|while|for|match|struct|type|async|await|spawn|return|true|false|enum|impl|trait|pub|use|const|in)\b)/
      );
      if (kwMatch) {
        tokens.push(
          <span key={key++} className="text-[#00ff88]">
            {kwMatch[1]}
          </span>
        );
        remaining = remaining.slice(kwMatch[1].length);
        continue;
      }

      // Type annotations (capitalized words after : or ->)
      const typeMatch = remaining.match(/^(\b[A-Z]\w*\b)/);
      if (typeMatch) {
        tokens.push(
          <span key={key++} className="text-purple-400">
            {typeMatch[1]}
          </span>
        );
        remaining = remaining.slice(typeMatch[1].length);
        continue;
      }

      // Function calls
      const fnCallMatch = remaining.match(/^(\b[a-z_]\w*)(\s*\()/);
      if (fnCallMatch) {
        tokens.push(
          <span key={key++} className="text-blue-400">
            {fnCallMatch[1]}
          </span>
        );
        tokens.push(<span key={key++}>{fnCallMatch[2]}</span>);
        remaining = remaining.slice(
          fnCallMatch[1].length + fnCallMatch[2].length
        );
        continue;
      }

      // Default: single character
      tokens.push(<span key={key++}>{remaining[0]}</span>);
      remaining = remaining.slice(1);
    }

    return (
      <React.Fragment key={lineIdx}>
        {lineIdx > 0 && "\n"}
        {tokens}
      </React.Fragment>
    );
  });
}

export default function CodeBlock({
  code,
  filename,
}: {
  code: string;
  filename?: string;
}) {
  return (
    <div className="rounded-xl border border-border overflow-hidden bg-surface">
      {filename && (
        <div className="px-4 py-2 border-b border-border text-xs text-gray-500 font-[family-name:var(--font-geist-mono)]">
          {filename}
        </div>
      )}
      <pre className="p-4 overflow-x-auto text-sm leading-relaxed font-[family-name:var(--font-geist-mono)] text-gray-300">
        <code>{highlightTurbo(code)}</code>
      </pre>
    </div>
  );
}
