# Programming Language Landscape: 2011-2026

> Research for new language design. All data gathered February 2026.
> Sources cited inline and collected at the end of each section.

---

## Table of Contents

1. [Language Popularity Trends (2011-2026)](#1-language-popularity-trends-2011-2026)
2. [New Project Adoption](#2-new-project-adoption)
3. [Developer Satisfaction](#3-developer-satisfaction)
4. [Ecosystem Growth](#4-ecosystem-growth)
5. [Job Market Data](#5-job-market-data)
6. [Key Trends](#6-key-trends)
7. [Implications for New Language Design](#7-implications-for-new-language-design)

---

## 1. Language Popularity Trends (2011-2026)

### 1.1 TIOBE Index

The TIOBE Index measures language popularity based on search engine query volume. It has been the longest-running popularity index, though its methodology (search engine hits) has well-known limitations.

#### TIOBE Top 10 Over Time

| Rank | 2011         | 2015         | 2018         | 2020         | 2022         | 2024         | Feb 2026       |
|------|--------------|--------------|--------------|--------------|--------------|--------------|----------------|
| 1    | Java         | Java         | Java         | C            | Python       | Python       | Python (21.81%)|
| 2    | C            | C            | C            | Java         | C            | C++          | C (11.05%)     |
| 3    | C++          | Objective-C  | Python       | Python       | Java         | Java         | C++ (8.55%)    |
| 4    | C#           | C++          | C++          | C++          | C++          | C            | Java (8.12%)   |
| 5    | PHP          | C#           | VB .NET      | C#           | C#           | C#           | C# (6.83%)     |
| 6    | Objective-C  | PHP          | C#           | VB           | VB           | JavaScript   | JavaScript (2.92%) |
| 7    | Python       | JavaScript   | JavaScript   | JavaScript   | JavaScript   | Go           | Visual Basic (2.85%) |
| 8    | Visual Basic | Python       | PHP          | PHP          | SQL          | Visual Basic | R (2.19%)      |
| 9    | JavaScript   | Perl         | SQL          | SQL          | Assembly     | SQL          | SQL (1.93%)    |
| 10   | Perl         | SQL          | Objective-C  | R            | PHP          | Fortran      | Delphi (1.88%) |

**Key trajectory observations:**
- **Python**: Rose from #7 (2011) to #1 (2022-2026). Peak of 26.98% in July 2025, declining to 21.81% by Feb 2026.
- **Java**: Fell from a decade of dominance (#1 in 2011-2018) to #4 by 2026.
- **C**: Remarkably resilient, remaining in the top 2-4 throughout the entire period.
- **C#**: Named TIOBE Language of the Year for 2025, achieving the largest year-over-year gain, reaching 6.83%.
- **Go**: Leaped from #13 (Jan 2024) to #7 (Jan 2025) -- its highest position ever.
- **Objective-C**: Dropped out of the top 20 entirely after Swift's introduction in 2014.

**TIOBE Language of the Year Awards (2011-2025):**

| Year | Language   | Year | Language   |
|------|------------|------|------------|
| 2011 | Objective-C| 2019 | C          |
| 2012 | Objective-C| 2020 | Python     |
| 2013 | Transact-SQL| 2021 | Python    |
| 2014 | JavaScript | 2022 | C++        |
| 2015 | Java       | 2023 | C#         |
| 2016 | Go         | 2024 | Python     |
| 2017 | C          | 2025 | C#         |
| 2018 | Python     |      |            |

> Sources: [TIOBE Index](https://www.tiobe.com/tiobe-index/), [TIOBE Index Wikipedia](https://en.wikipedia.org/wiki/TIOBE_index), [TechRepublic - TIOBE Feb 2026](https://www.techrepublic.com/article/news-tiobe-index-language-rankings/), [InfoWorld - Python slipping](https://www.infoworld.com/article/4129615/python-is-slipping-in-popularity-tiobe.html), [ADTmag - C# wins 2025](https://adtmag.com/articles/2026/01/07/tiobe-language-rankings-for-2025.aspx)

---

### 1.2 GitHub Octoverse (Top Languages by Contributors/Repos)

GitHub Octoverse tracks the most-used languages by repository contributions, pull requests, and contributor counts.

| Year | #1          | #2         | #3          | #4       | #5       | Notable                                      |
|------|-------------|------------|-------------|----------|----------|----------------------------------------------|
| 2014 | JavaScript  | Java       | Ruby        | PHP      | Python   | Ruby still in top 3                           |
| 2017 | JavaScript  | Java       | Python      | Ruby     | PHP      | Python overtakes Ruby                         |
| 2019 | JavaScript  | Python     | Java        | PHP      | C#       | Python moves to #2                            |
| 2022 | JavaScript  | Python     | Java        | TypeScript| C#      | TypeScript enters top 5                       |
| 2023 | JavaScript  | Python     | TypeScript  | Java     | C#       | TypeScript overtakes Java (37% user growth)   |
| 2024 | Python      | JavaScript | TypeScript  | Java     | C#       | Python overtakes JavaScript for #1            |
| 2025 | TypeScript  | Python     | JavaScript  | Java     | C#       | TypeScript hits #1 (2.64M monthly contributors, +66.6% YoY) |

**Key shifts:**
- TypeScript's rise has been meteoric: not in the top 10 in 2014, to #1 by August 2025.
- Python surpassed JavaScript for the first time in 2024, driven by AI/ML.
- TypeScript then surpassed both in 2025, growing by 1.05 million contributors in a single year.
- JavaScript's relative decline reflects migration to TypeScript, not abandonment of the ecosystem.

> Sources: [GitHub Octoverse 2025](https://octoverse.github.com/), [InfoWorld - TypeScript rises](https://www.infoworld.com/article/4080454/typescript-rises-to-the-top-on-github.html), [Visual Studio Magazine - TypeScript tops Octoverse](https://visualstudiomagazine.com/articles/2025/10/31/typescript-tops-github-octoverse-as-ai-era-reshapes-language-choices.aspx), [InfoWorld - Python overtakes JS](https://www.infoworld.com/article/3594587/python-has-overtaken-javascript-on-github.html)

---

### 1.3 Stack Overflow Developer Survey

The Stack Overflow Developer Survey is the largest annual survey of developers worldwide. In 2023, it shifted from "Loved/Dreaded/Wanted" to "Admired/Desired" terminology.

#### Most Used Languages (% of respondents)

| Language     | 2019  | 2020  | 2021  | 2022  | 2023  | 2024  | 2025  |
|-------------|-------|-------|-------|-------|-------|-------|-------|
| JavaScript  | 67.8% | 67.7% | 64.9% | 65.4% | 63.6% | 62.3% | 66.0% |
| HTML/CSS    | 63.5% | 63.1% | 56.1% | 52.3% | 52.9% | 52.6% | ~55%  |
| Python      | 41.7% | 44.1% | 48.2% | 48.1% | 49.3% | 51.0% | ~58%  |
| SQL         | 54.4% | 54.7% | 47.1% | 49.4% | 48.7% | 49.0% | ~50%  |
| TypeScript  | 21.2% | 25.4% | 30.2% | 34.8% | 38.9% | 38.5% | ~40%  |
| Java        | 41.1% | 40.2% | 35.4% | 33.3% | 30.5% | 30.3% | ~28%  |
| Rust        |  3.2% |  5.1% |  7.0% |  9.3% | 13.1% | 12.6% | ~14%  |
| Go          |  8.2% |  9.4% |  9.6% | 11.2% | 13.2% | 13.5% | ~14%  |

#### Most Loved/Admired Languages

| Language   | 2019   | 2020   | 2021   | 2022   | 2023*  | 2024*  | 2025*  |
|-----------|--------|--------|--------|--------|--------|--------|--------|
| Rust      | 83.5%  | 86.1%  | 86.7%  | 87.0%  | 84.6%  | 83.5%  | 72.0%  |
| Clojure   | 68.3%  | 68.3%  | 72.2%  | 75.2%  | 68.5%  | 67.8%  |   --   |
| TypeScript| 73.1%  | 67.1%  | 72.7%  | 73.5%  | 71.7%  | 70.5%  |   --   |
| Elixir    | 68.2%  | 70.1%  | 73.0%  | 75.0%  | 73.1%  | 70.0%  | 66.0%  |
| Go        | 67.9%  | 62.3%  | 62.0%  | 64.6%  | 62.5%  | 60.2%  |   --   |
| Gleam     |   --   |   --   |   --   |   --   |   --   |   --   | 70.0%  |
| Zig       |   --   |   --   |   --   |   --   |   --   |   --   | 64.0%  |

*2023+ uses "Admired" methodology (% who used it and want to continue).

**Key observations:**
- **Rust** has been #1 in loved/admired for 8+ consecutive years (2016-2025).
- **Gleam** appeared for the first time in 2025 at an impressive 70% admiration.
- **Zig** debuted at 64% admiration in 2025, signaling interest in new systems languages.
- **Python** saw the largest year-over-year usage increase: +7 percentage points from 2024 to 2025.

#### Most Dreaded Languages (2022, last year of this terminology)

| Language      | Dreaded % |
|---------------|-----------|
| MATLAB        | 80.5%     |
| COBOL         | 78.2%     |
| VBA           | 75.5%     |
| Objective-C   | 73.2%     |
| Assembly      | 71.4%     |
| Perl          | 70.5%     |
| PHP           | 60.1%     |

> Sources: [SO Survey 2025](https://survey.stackoverflow.co/2025/technology), [SO Survey 2024](https://survey.stackoverflow.co/2024/technology), [SO Survey 2023](https://survey.stackoverflow.co/2023/), [SO Blog - 2024 results](https://stackoverflow.blog/2025/01/01/developers-want-more-more-more-the-2024-results-from-stack-overflow-s-annual-developer-survey/)

---

### 1.4 RedMonk Rankings

RedMonk combines GitHub pull requests with Stack Overflow discussion to produce a composite ranking. Their methodology is uniquely positioned to measure both usage and community engagement.

#### RedMonk Top 20 (January 2025)

| Rank | Language     | Rank | Language    |
|------|-------------|------|-------------|
| 1    | JavaScript  | 11   | Swift       |
| 2    | Python      | 12   | R           |
| 3    | Java        | 13   | Objective-C |
| 4    | PHP         | 14   | Scala       |
| 5    | C#          | 15   | Go          |
| 6    | TypeScript  | 16   | Shell       |
| 7    | CSS         | 17   | PowerShell  |
| 7    | C++         | 18   | Kotlin      |
| 9    | Ruby        | 19   | Rust        |
| 10   | C           | 20   | Dart        |

**Notable:** The top 3 (JavaScript, Python, Java) have been unchanged since 2018. TypeScript has been steadily climbing. Rust is at #19, still relatively low in absolute usage despite massive developer enthusiasm.

> Sources: [RedMonk Jan 2025](https://redmonk.com/sogrady/2025/06/18/language-rankings-1-25/), [RedMonk Jan 2024](https://redmonk.com/sogrady/2024/03/08/language-rankings-1-24/), [RedMonk Top 20 Over Time](https://redmonk.com/rstephens/2025/06/18/top20-jan2025/)

---

## 2. New Project Adoption

### 2.1 Greenfield vs Legacy Language Choices

The distinction between languages chosen for new ("greenfield") projects versus legacy maintenance is critical for understanding where the industry is heading.

#### Languages Predominantly Chosen for New Projects (2024-2025)

| Domain                      | Primary Choices            | Rising Alternatives       |
|-----------------------------|----------------------------|---------------------------|
| Web Frontend                | TypeScript, JavaScript     | Rust (via WASM)           |
| Web Backend (API)           | TypeScript/Node, Go, Python| Rust, Elixir              |
| Mobile (iOS)                | Swift                      | Kotlin Multiplatform      |
| Mobile (Android)            | Kotlin                     | Kotlin Multiplatform      |
| Mobile (Cross-platform)     | Dart/Flutter, React Native | Kotlin Multiplatform      |
| Data Science / ML           | Python                     | Julia, Mojo               |
| Systems / Infrastructure    | Rust, Go, C++              | Zig                       |
| Cloud Native / Microservices| Go, Java (Spring Boot)     | Rust, TypeScript          |
| CLI Tools                   | Go, Rust                   | Zig                       |
| DevOps / Scripting          | Python, Bash, Go           | Deno (TypeScript)         |
| Blockchain / Web3           | Rust (Solana), Solidity    | Move                      |
| Game Development            | C++, C#                    | Rust (Bevy), Zig          |
| Embedded / IoT              | C, C++                     | Rust, Zig                 |

#### Languages Primarily in Legacy/Maintenance Mode

| Language     | Legacy Domain                        | Migration Target         |
|-------------|--------------------------------------|--------------------------|
| PHP         | WordPress, older web apps            | TypeScript/Node, Go      |
| Perl        | Sys admin scripts, bioinformatics    | Python, Go               |
| Ruby        | Rails apps (2008-2016 era)           | Go, TypeScript, Elixir   |
| Objective-C | iOS apps pre-2014                    | Swift                    |
| Java (EE)   | Enterprise monoliths                 | Kotlin, Go, TypeScript   |
| VB.NET      | Legacy Windows desktop               | C#                       |
| COBOL       | Banking/finance mainframes           | Java, Python (wrappers)  |
| ColdFusion  | Legacy web applications              | Any modern stack          |

### 2.2 Framework-Driven Language Selection

Modern language adoption is increasingly driven by framework ecosystems rather than language features alone:

| Framework/Platform     | Language    | Growth Signal                            |
|-----------------------|-------------|------------------------------------------|
| Next.js / Remix       | TypeScript  | Default scaffolding now uses TypeScript   |
| FastAPI               | Python      | Fastest-growing Python web framework      |
| Axum / Actix          | Rust        | Growing adoption for high-perf backends   |
| Gin / Echo            | Go          | Standard choice for Go microservices      |
| SvelteKit             | TypeScript  | Growing alternative to React              |
| Bevy                  | Rust        | Emerging game engine                      |
| Tauri                 | Rust+TS     | Growing Electron alternative              |
| Zig + build system    | Zig         | Drop-in C/C++ toolchain replacement       |

> Sources: [Daily.dev - Language Trends 2025](https://business.daily.dev/resources/programming-language-trends-what-developers-are-using-now/), [Semaphore - Emerging Languages 2025](https://semaphore.io/blog/programming-languages-2025), [Accio - 2025 Trends](https://www.accio.com/business/programming-language-trends)

---

## 3. Developer Satisfaction

### 3.1 Satisfaction Rankings Across Surveys

Developer satisfaction is measured differently by different surveys. Here is a composite view:

#### Highest Satisfaction Languages (2024-2025 composite)

| Language   | SO Admired 2025 | JetBrains "Promise Index" | Key Satisfaction Drivers                     |
|-----------|-----------------|---------------------------|----------------------------------------------|
| Rust      | 72%             | Top 3                     | Memory safety without GC, expressive type system, tooling (cargo), community |
| Gleam     | 70%             | --                        | Type safety, fault tolerance (BEAM VM), simplicity |
| TypeScript| ~68% (2024)     | #1                        | Type safety on JS ecosystem, IDE support, gradual adoption |
| Elixir    | 66%             | --                        | Concurrency model, fault tolerance, developer ergonomics |
| Zig       | 64%             | --                        | Simplicity, C interop, compile-time evaluation |
| Go        | ~60%            | Top 3                     | Simplicity, fast compilation, excellent stdlib, concurrency |
| Kotlin    | ~60%            | Growing                   | Modern Java alternative, multiplatform, null safety |
| Python    | ~55%            | High usage                | Ecosystem breadth, readability, AI/ML dominance |
| Swift     | ~55%            | Stable                    | Apple ecosystem, safety, modern syntax        |

#### JetBrains Language Promise Index (2024)

JetBrains introduced a "Language Promise Index" that measures a language's growth trajectory and future potential:

| Rank | Language   | Signal                                         |
|------|------------|------------------------------------------------|
| 1    | TypeScript | Fastest real-world usage growth over 5 years   |
| 2    | Rust       | Steady market share growth, high satisfaction   |
| 3    | Go         | Cloud-native adoption accelerating             |
| 4    | Kotlin     | Multiplatform expansion beyond Android          |
| 5    | Python     | AI-driven surge, but approaching saturation     |

#### Languages at "Maturity Plateau" (JetBrains 2025)

These languages have reached their ceiling in terms of growth potential:
- JavaScript (market share stable/declining as TS absorbs it)
- PHP (stable in legacy, declining in new projects)
- SQL (ubiquitous but not growing)
- Ruby (stable but not growing)

### 3.2 Why Developers Love What They Love

Based on survey data and community analysis, the satisfaction drivers cluster into these themes:

| Factor                        | Languages That Excel        | Languages That Struggle      |
|-------------------------------|----------------------------|------------------------------|
| **Type safety**               | Rust, TypeScript, Kotlin   | JavaScript, Python, PHP      |
| **Tooling quality**           | Rust (cargo), Go, TypeScript| C, C++, Java (historically) |
| **Error handling**            | Rust (Result), Go, Elixir  | Java (exceptions), JS       |
| **Build speed**               | Go, Zig                    | Rust, C++, Scala             |
| **Runtime performance**       | Rust, C++, Zig, Go         | Python, Ruby, JS             |
| **Ecosystem breadth**         | Python, JavaScript/TS, Java| Rust, Zig, Gleam             |
| **Learning curve**            | Python, Go, JavaScript     | Rust, Haskell, C++           |
| **Concurrency model**         | Go, Rust, Elixir, Erlang   | Python (GIL), Ruby           |
| **Memory safety**             | Rust, Go, Java, C#         | C, C++                       |
| **Community & documentation** | Rust, Python, Go           | Zig, Gleam (still growing)   |

> Sources: [SO Survey 2025](https://survey.stackoverflow.co/2025/), [JetBrains DevEco 2024](https://www.jetbrains.com/lp/devecosystem-2024/), [JetBrains DevEco 2025](https://blog.jetbrains.com/research/2025/10/state-of-developer-ecosystem-2025/), [Visual Studio Magazine - JetBrains Promise Index](https://visualstudiomagazine.com/articles/2024/12/11/typescript-tops-new-jetbrains-language-promise-index.aspx)

---

## 4. Ecosystem Growth

### 4.1 Package Registry Growth

| Registry        | Language(s)    | ~2018       | ~2021          | ~2024          | ~2025-2026     | Growth Factor  |
|----------------|---------------|-------------|----------------|----------------|----------------|----------------|
| **npm**        | JS/TS         | ~700K       | ~1.8M          | ~2.5M+         | ~3.0M+         | ~4.3x (7yr)    |
| **PyPI**       | Python        | ~130K       | ~350K          | ~530K          | ~850K+         | ~6.5x (7yr)    |
| **crates.io**  | Rust          | ~20K        | ~70K           | ~150K          | ~200K+         | ~10x (7yr)     |
| **Maven Central** | Java/Kotlin| ~300K       | ~500K          | ~700K+         | ~850K+         | ~2.8x (7yr)    |
| **NuGet**      | C#/.NET       | ~130K       | ~280K          | ~400K+         | ~450K+         | ~3.5x (7yr)    |
| **Go Modules** | Go            | (pre-modules)| ~350K         | ~500K+         | ~600K+         | N/A            |
| **RubyGems**   | Ruby          | ~150K       | ~170K          | ~180K          | ~185K          | ~1.2x (7yr)    |
| **Cargo** downloads | Rust     | --          | ~10B cumul.    | ~40B+ cumul.   | ~60B+ cumul.   | Accelerating   |

**Key observations:**
- **crates.io** has the highest relative growth rate (~10x in 7 years), though from a smaller base.
- **PyPI** is experiencing explosive growth (~6.5x), fueled by the AI/ML library explosion.
- **npm** remains the largest registry by raw count (3M+ packages), though significant spam/abandoned packages exist. By 2023, 52.8% of new crates on crates.io were never updated, and npm faces similar issues.
- **RubyGems** growth has essentially flatlined, reflecting Ruby's plateau.
- The TypeScript compiler alone exceeds 60 million npm downloads per week (Q1 2025), up from 20 million in 2021.

### 4.2 PyPI 2025 Year in Review

- 3.9 million new files published
- 130,000 new projects created
- 52% two-factor authentication adoption among active maintainers
- Total packages approaching 850,000

### 4.3 crates.io Ecosystem Characteristics (2025)

- 200,650 total crates as of October 2025
- Among crates with 10,000+ downloads: average time since last update is 771 days (median: 454 days)
- Crates surviving beyond 7 years show strong maintenance and stability
- 52.8% of new crates in 2025 are single-publish experiments (vs 1.4% in 2015)

> Sources: [PyPI 2025 Year in Review](https://blog.pypi.org/posts/2025-12-31-pypi-2025-in-review/), [crates.io stats (lib.rs)](https://lib.rs/stats), [State of the Crates 2025](https://ohadravid.github.io/posts/2024-12-state-of-the-crates/), [npm Retrospective 2023](https://socket.dev/blog/2023-npm-retrospective)

---

## 5. Job Market Data

### 5.1 Language Demand (Job Postings)

Based on DevJobsScanner analysis of 14M+ job postings (Jan 2023 - Sep 2024):

| Rank | Language           | Job Postings | % of Market |
|------|--------------------|-------------|-------------|
| 1    | JavaScript/TypeScript| 651K       | ~31%        |
| 2    | Python             | 408K        | ~20%        |
| 3    | Java               | 362K        | ~17%        |
| 4    | C#                 | 200K+       | ~10%        |
| 5    | C/C++              | 180K+       | ~9%         |
| 6    | PHP                | 120K+       | ~6%         |
| 7    | Go                 | 80K+        | ~4%         |
| 8    | Rust               | 25K+        | ~1.2%       |

**Key insight:** JavaScript/TypeScript dominates job postings by a wide margin. Rust demand is growing rapidly but still represents a small fraction of total job listings.

### 5.2 Salary Data by Language (US Market, 2025)

| Language     | Average (USD/yr) | Entry-Level   | Senior/Expert  | Trend     |
|-------------|------------------|---------------|----------------|-----------|
| Rust        | $150,000         | $130,000      | $195,000+      | Rising    |
| Scala       | $146,664         | $115,000      | $175,000+      | Stable    |
| Go          | $137,500         | $120,000      | $180,000+      | Rising    |
| TypeScript  | $131,956         | $114,206      | $163,280       | Rising    |
| Clojure     | $129,348         | $106,000      | $200,000       | Stable    |
| C++         | $129,571         | $100,000      | $249,970       | Stable    |
| Python      | $125,740         | $105,206      | $157,607       | Rising    |
| Java        | $120,000         | $95,000       | $150,000       | Stable    |
| JavaScript  | $117,002         | $97,029       | $154,956       | Stable    |
| C#          | $115,000         | $90,000       | $145,000       | Stable    |
| PHP         | $98,000          | $75,000       | $130,000       | Declining |

**Salary premium pattern:** Languages with smaller supply relative to demand (Rust, Scala, Go, Clojure) command higher salaries. The "scarcity premium" effect is clear: Rust developers are in high demand but low supply.

### 5.3 Supply vs Demand Imbalance

| Language   | Demand Growth | Developer Supply | Imbalance        |
|-----------|---------------|------------------|------------------|
| Rust      | High (+40% YoY)| Low (growing)   | Severe shortage  |
| Go        | High (+25% YoY)| Medium          | Moderate shortage|
| Python    | Very High      | Very High        | Balanced         |
| TypeScript| Very High      | High (growing)   | Slight shortage  |
| JavaScript| Stable         | Very High        | Oversupplied     |
| Java      | Declining      | Very High        | Oversupplied     |
| PHP       | Declining      | High             | Oversupplied     |

> Sources: [DevJobsScanner - Top 8 Languages](https://www.devjobsscanner.com/blog/top-8-most-demanded-programming-languages/), [VentureBeat - Best Paid Languages](https://venturebeat.com/programming-development/these-are-the-best-paid-programming-languages-for-2025), [Index.dev - Highest Paying](https://www.index.dev/blog/highest-paying-programming-languages), [Phaedra Solutions - Salaries](https://www.phaedrasolutions.com/blog/highest-paying-programming-languages), [GeeksforGeeks - Salaries](https://www.geeksforgeeks.org/blogs/highest-paying-programming-languages/), [Statista - Recruiter Demand](https://www.statista.com/statistics/1296727/programming-languages-demanded-by-recruiters/)

---

## 6. Key Trends

### 6.1 The Rise of Rust

**Timeline:**
| Year | Milestone                                                        |
|------|------------------------------------------------------------------|
| 2010 | Rust announced by Graydon Hoare at Mozilla                       |
| 2015 | Rust 1.0 released; first year winning SO "most loved"            |
| 2016 | Firefox Servo components written in Rust                         |
| 2018 | Rust adoption begins outside Mozilla (AWS, Microsoft)            |
| 2020 | AWS releases Firecracker (microVM) written entirely in Rust      |
| 2021 | Rust Foundation formed (AWS, Google, Huawei, Microsoft, Mozilla) |
| 2022 | Linux kernel accepts Rust as a second language (alongside C)     |
| 2023 | Android team uses Rust for system components; adoption at 13.1% SO usage |
| 2024 | White House recommends memory-safe languages, citing Rust        |
| 2025 | Company adoption up 4% YoY; 45% of orgs making "non-trivial" use; 200K+ crates |

**Adoption metrics:**
- Professional usage: 38% use Rust for majority of work coding (up from 34% previous year)
- 53% of Rust developers consider themselves productive (up from 47% in 2023)
- Automotive Rust market: $428M (2024), projected $2.1B by 2033 (19.2% CAGR)
- GitHub Rust stars grew 15% in 2024
- 68.75% growth in commercial usage between 2021 and 2025
- 30% of 2025 survey respondents started using Rust less than a month ago

**Why Rust matters for new language design:**
- Proved that a language can be safe AND fast without garbage collection
- Demonstrated that an algebraic type system can be practical for industry
- Showed that excellent tooling (cargo, rustfmt, clippy) is a first-class concern
- But: compile times and learning curve remain persistent criticisms

> Sources: [ZenRows - Rust Popularity 2025](https://www.zenrows.com/blog/rust-popularity), [Yalantis - Rust Market](https://yalantis.com/blog/rust-market-overview/), [Rust Blog - 2024 Survey](https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/), [JetBrains - State of Rust 2025](https://blog.jetbrains.com/rust/2026/02/11/state-of-rust-2025/)

---

### 6.2 TypeScript Explosion

**Growth trajectory:**
| Year | Key Metric                                                          |
|------|---------------------------------------------------------------------|
| 2012 | TypeScript 0.8 released by Microsoft                                |
| 2014 | Angular 2 bets on TypeScript, providing first major adoption driver |
| 2017 | 12% developer adoption (JetBrains)                                  |
| 2018 | VSCode (written in TS) becomes most popular editor                  |
| 2020 | 1.6M public GitHub repos; 20M weekly npm downloads of tsc           |
| 2022 | Deno and Bun provide native TypeScript runtimes                     |
| 2023 | Overtakes Java on GitHub (#3); 37% user base growth                 |
| 2024 | 35% developer adoption (JetBrains); 400%+ enterprise growth since 2020 |
| 2025 | #1 on GitHub (2.64M contributors, +66.6% YoY); 60M+ weekly tsc downloads; 4.2M repos |

**Why TypeScript exploded:**
- **Gradual typing**: Can be adopted incrementally in existing JS projects
- **Ecosystem leverage**: Full access to npm and the JavaScript ecosystem
- **IDE experience**: Type information enables superior autocomplete, refactoring, navigation
- **AI compatibility**: Typed code works better with AI code assistants (Copilot, etc.)
- **Framework defaults**: Next.js, SvelteKit, Angular all now default to TypeScript scaffolding
- **Runtime expansion**: Deno, Bun treat TypeScript as first-class (no compile step)

**Lesson for new language design:** TypeScript proved that incremental adoption through an existing ecosystem is one of the most powerful adoption strategies a language can have.

> Sources: [Codecademy - TS Most Used](https://www.codecademy.com/resources/blog/typescript-most-used-language-on-github), [JetBrains - JS/TS Trends 2024](https://blog.jetbrains.com/webstorm/2024/02/js-and-ts-trends-2024/), [Visual Studio Magazine - JetBrains](https://visualstudiomagazine.com/articles/2023/02/02/jetbrains-survey.aspx), [Index.dev - TS vs JS](https://www.index.dev/blog/javascript-vs-typescript-popularity)

---

### 6.3 Python's AI-Driven Surge

**The AI multiplier effect:**
| Year | Python TIOBE % | AI Milestone                                          |
|------|----------------|-------------------------------------------------------|
| 2017 | ~5%            | TensorFlow 1.0, PyTorch 0.1 released                 |
| 2019 | ~9%            | Hugging Face Transformers library launched            |
| 2020 | ~11%           | GPT-3 released; Python becomes default AI language    |
| 2022 | ~15%           | Stable Diffusion, ChatGPT; Python #1 on TIOBE        |
| 2023 | ~16%           | LLM explosion; LangChain, LlamaIndex emerge          |
| 2024 | ~23%           | AI adoption jumps to 78% of businesses               |
| 2025 | ~27% (peak)    | AI coding tools used by 85% of devs; Python peaks    |
| 2026 | ~22% (Feb)     | Slight decline as specialized languages gain ground   |

**Python's AI ecosystem:**
- **ML Frameworks**: TensorFlow, PyTorch, JAX, scikit-learn
- **LLM Tools**: LangChain, LlamaIndex, Hugging Face, OpenAI SDK
- **Data Processing**: pandas, NumPy, Polars, Dask
- **Notebooks**: Jupyter (universal standard for data science)
- **Deployment**: FastAPI, Streamlit, Gradio

**Emerging challenge**: Python reached peak popularity at 26.98% (July 2025) and has since declined to 21.81%. More specialized languages (R, Mojo, Julia) are gaining ground in specific niches. Performance limitations for production AI inference remain a pain point.

> Sources: [TIOBE Index](https://www.tiobe.com/tiobe-index/), [JetBrains Python Survey 2024](https://lp.jetbrains.com/python-developers-survey-2024/), [JetBrains State of Python 2025](https://blog.jetbrains.com/pycharm/2025/08/the-state-of-python-2025/), [DigitalOcean AI Statistics](https://www.digitalocean.com/resources/articles/artificial-intelligence-statistics)

---

### 6.4 Go's Cloud Dominance

**Cloud-native infrastructure built in Go:**

| Project       | Category              | GitHub Stars (approx) |
|---------------|-----------------------|-----------------------|
| Kubernetes    | Container orchestration| 110K+                |
| Docker (Moby) | Container runtime     | 69K+                 |
| Prometheus    | Monitoring            | 56K+                 |
| Grafana       | Observability         | 65K+                 |
| Terraform     | Infrastructure as Code| 43K+                 |
| etcd          | Distributed KV store  | 48K+                 |
| CockroachDB   | Distributed SQL DB    | 30K+                 |
| Istio         | Service mesh          | 36K+                 |
| Traefik       | Reverse proxy         | 52K+                 |
| CoreDNS       | DNS server            | 12K+                 |

**Go adoption metrics (2024-2025):**
- ~5.8 million Go developers worldwide (2024)
- 11% of all developers planning to adopt Go in next 12 months (JetBrains 2025)
- 49% of backend developers work with cloud-native architectures (~9.2M specialists)
- Go accounts for 12% of all API calls on Cloudflare (up from 8.4% previous year)
- 18,000+ companies using Go in production
- TIOBE #7 (Jan 2025) -- highest position ever for Go

**Why Go dominates cloud:**
- **Fast compilation**: Seconds, not minutes
- **Small binaries**: Ideal for containers
- **Built-in concurrency**: Goroutines and channels
- **Simple language**: 25 keywords, minimal syntax
- **Excellent standard library**: HTTP server, JSON, crypto, all built in
- **Static binary deployment**: No runtime dependencies

> Sources: [ZenRows - Go Popularity](https://www.zenrows.com/blog/golang-popularity), [JetBrains - Go Trends 2025](https://blog.jetbrains.com/go/2025/11/10/go-language-trends-ecosystem-2025/), [JetBrains - Go Growth 2024](https://blog.jetbrains.com/research/2025/04/is-golang-still-growing-go-language-popularity-trends-in-2024/), [OpenSourceForU - Go Cloud Native](https://www.opensourceforu.com/2025/11/go-driving-the-next-wave-of-cloud-native-infrastructure/)

---

### 6.5 WebAssembly (WASM) Emergence

**Evolution timeline:**

| Year | Milestone                                                         |
|------|-------------------------------------------------------------------|
| 2015 | WebAssembly announced as collaboration between browser vendors    |
| 2017 | WASM 1.0 ships in all major browsers                              |
| 2019 | WASI (WebAssembly System Interface) proposed for server-side use  |
| 2020 | Figma uses WASM for its design tool rendering engine              |
| 2021 | Docker announces WASM support; "if WASM existed in 2008..."      |
| 2023 | WASM Component Model proposal; 41% of surveyed devs use in prod  |
| 2024 | WASM cloud market: $1.36B; Spin, Fermyon, Fastly edge compute    |
| 2025 | WASM 3.0: GC, 64-bit memory, exception handling; AmEx largest commercial deployment |
| 2026 | Market projected at $5.74B by 2029 (33.3% CAGR)                  |

**Current WASM adoption metrics (2025):**
- 4.5% of web applications use WASM, projected 50% by 2030
- ~5.5% of Chrome page loads involve WASM (Chrome Platform Status)
- 41% of developers using WASM in production; 28% piloting or planning
- WASM 3.0 now supports GC languages (Java, Kotlin, Dart, Scala)

**Key WASM use cases:**
- **Browser compute**: Figma, AutoCAD Web, Google Earth, Photoshop Web
- **Serverless/Edge**: Cloudflare Workers, Fastly Compute, Fermyon Spin
- **Plugins**: Envoy proxy filters, Shopify Functions, Zed editor extensions
- **Containers alternative**: American Express FaaS platform (largest commercial deployment)
- **Video/Audio**: Zoom, Google Meet client-side processing

**Languages compiling to WASM:**

| Language | WASM Support Quality | Notes                        |
|----------|---------------------|------------------------------|
| Rust     | Excellent           | Most mature WASM story       |
| C/C++    | Excellent           | Via Emscripten               |
| Go       | Good                | TinyGo for smaller binaries  |
| AssemblyScript | Good          | TypeScript-like syntax       |
| Zig      | Good                | Growing WASM target support  |
| Python   | Experimental        | Via Pyodide                  |
| Kotlin   | Improving           | Via Kotlin/Wasm (WasmGC)     |
| Java     | Improving           | Via TeaVM, GraalWasm          |
| Swift    | Experimental        | SwiftWasm project            |

> Sources: [HTTP Archive Web Almanac 2025 - WASM](https://almanac.httparchive.org/en/2025/webassembly), [Uno Platform - State of WASM 2024-2025](https://platform.uno/blog/state-of-webassembly-2024-2025/), [Uno Platform - State of WASM 2025-2026](https://platform.uno/blog/the-state-of-webassembly-2025-2026/), [ByteIota - WASM Adoption](https://byteiota.com/webassembly-hits-4-5-adoption-eyes-50-by-2030/)

---

### 6.6 Other Notable Trends

#### The AI Coding Revolution
- 85% of developers regularly use AI tools for coding (JetBrains 2025)
- 62% rely on at least one AI coding assistant daily
- 49% plan to try AI coding agents in 2025
- Typed languages (TypeScript, Rust) work better with AI assistants, accelerating their adoption
- GitHub Copilot now generates ~46% of code in files where it's active

#### The Rise of New Systems Languages
- **Zig**: First appeared in SO 2025 at 64% admiration; Andrew Kelley's vision of "C without the footguns"
- **Mojo**: Python superset targeting AI/ML with systems-level performance
- **Vale**: Experimental language exploring new memory management approaches
- **Carbon**: Google's "successor to C++" experimental language

#### Functional Programming Influence
- Algebraic data types now in Rust, Swift, Kotlin, TypeScript (discriminated unions)
- Pattern matching added to Python 3.10 (2021), Java 21 (2023), C# 7+ (ongoing)
- Result/Option types replacing exceptions (Rust influence spreading)
- Immutability-by-default gaining traction as a design philosophy

---

## 7. Implications for New Language Design

Based on the 15-year landscape analysis, these are the key signals for designing a new programming language:

### What the Market Rewards

| Signal                                      | Evidence                                                |
|---------------------------------------------|---------------------------------------------------------|
| **Type safety is now expected**             | TypeScript, Rust, Kotlin all grew by adding types       |
| **Tooling is as important as syntax**       | Cargo (Rust), go toolchain, npm/tsc ecosystem           |
| **Fast compile times matter**               | Go's popularity partly due to instant compilation       |
| **Memory safety is table stakes**           | White House recommendation; Rust, Go, Java all safe     |
| **Ecosystem leverage wins**                 | TypeScript's #1 strategy was "all of npm for free"      |
| **AI assistant compatibility**              | Typed languages with clear semantics work better with AI|
| **Gradual adoption paths**                  | TypeScript proved incremental migration beats rewrite   |
| **Concurrency must be first-class**         | Go goroutines, Rust async, Elixir actors                |
| **WASM as a compilation target**            | Cross-platform deployment increasingly through WASM     |
| **Developer experience over raw features**  | Go's simplicity beats C++'s power for most use cases    |

### Underserved Niches (Opportunities)

| Niche                                        | Current Pain Point                                      |
|----------------------------------------------|---------------------------------------------------------|
| Systems programming with fast compile times  | Rust compiles slowly; C/C++ are unsafe                  |
| Python-speed development with native perf    | Python is slow; Rust has steep learning curve            |
| Type-safe scripting                          | Bash is untyped; Python typing is optional/inconsistent |
| Full-stack single language (beyond JS/TS)    | Go/Rust lack browser story; TS lacks systems perf       |
| AI-native language (not just libraries)      | Python is the default but has fundamental perf limits    |
| WASM-first language design                   | Current languages compile to WASM as afterthought       |

### Anti-Patterns to Avoid

| Anti-Pattern                               | Cautionary Example                                     |
|-------------------------------------------|--------------------------------------------------------|
| No ecosystem / "build everything from scratch" | D, Nim struggled despite good designs               |
| Too complex / too many features            | C++ complexity drives developers to Go and Rust        |
| No gradual adoption story                  | Languages requiring full rewrite see slow adoption     |
| Poor error messages                        | C++ template errors are legendary for their opacity    |
| Ignoring tooling (formatter, linter, LSP)  | Languages without LSP support lose IDE-era developers  |
| No clear use case / "general purpose only" | Languages need a compelling "killer app" domain         |

---

*This document was compiled in February 2026 as foundational research for new programming language design. Data points are sourced from TIOBE, GitHub Octoverse, Stack Overflow Developer Survey, RedMonk, JetBrains State of Developer Ecosystem, and official package registry statistics. All sources are cited inline.*
