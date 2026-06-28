"use client";

import { useState } from "react";
import CodeBlock from "./code-block";

interface Tab {
  label: string;
  filename?: string;
  code: string;
}

export default function CodeTabs({ tabs }: { tabs: Tab[] }) {
  const [active, setActive] = useState(0);

  return (
    <div>
      <div className="flex gap-1 mb-4">
        {tabs.map((tab, i) => (
          <button
            key={tab.label}
            onClick={() => setActive(i)}
            className={`px-4 py-2 text-sm rounded-lg transition-colors font-[family-name:var(--font-geist-sans)] ${
              active === i
                ? "bg-surface text-accent border border-border"
                : "text-gray-400 hover:text-gray-300"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <CodeBlock code={tabs[active].code} filename={tabs[active].filename} />
    </div>
  );
}
