# Carl Code — Multi-Agent CLI Tool

**Date:** 2026-04-09
**Status:** Approved
**Built with:** Turbo Lang (no compiler changes)
**Repo:** Standalone (separate from Turbo Lang)

---

## 1. Overview

Carl Code is a multi-agent CLI tool written entirely in Turbo Lang. The user talks to Carl, an assistant agent who analyzes tasks and assembles dynamic squads of specialist agents to handle them. Agents have real names, distinct personalities, philosophies, and speech styles. They communicate with each other, disagree, and synthesize results.

The product serves two purposes:
1. A real, useful multi-agent coding tool
2. The flagship "look what you can build" example for Turbo Lang

**No Turbo Lang compiler changes required.** This is pure application code using existing primitives: `agent`, `spawn`/`await`, `channel`, structs, closures, `read_file`/`write_file`/`http_get`/`http_post`.

## 2. Architecture

### Interaction Model

```
User Input (read_line)
    │
    ▼
Carl (Assistant Agent) — always running, manages conversation
    │
    ├─ Analyzes task
    ├─ Picks squad members from agent roster
    ├─ spawn() each agent with task context + channel
    │
    ▼
Squad (2-5 agents, dynamic per task)
    │
    ├─ Each agent runs concurrently (spawn/await)
    ├─ Agents communicate via channels (intra-squad)
    ├─ Each agent calls .ask() with their persona + task
    ├─ Agents can invoke tools (file ops, shell, search)
    │
    ▼
Carl collects results, synthesizes, presents to user
    │
    ▼
User sees: Carl's summary + individual agent contributions
           (with each agent's name, color, and voice)
```

### Dynamic Squad Assembly

Carl does NOT use a fixed hierarchy. For each task, Carl:
1. Analyzes the user's request
2. Selects 2-5 agents whose skills match the task
3. Spawns them concurrently with shared context
4. Agents can message each other within the squad via channels
5. Carl collects all outputs, resolves conflicts, synthesizes a response

This showcases Turbo's `spawn`/`await` and `channel` primitives in a real application.

### Key Data Structures

```turbo
struct AgentProfile {
    name: str,
    role: str,
    philosophy: str,
    speech_style: str,
    skills: [str],
    color: str,           // ANSI color code
    system_prompt: str
}

struct Squad {
    agents: [AgentProfile],
    task: str,
    context: str,
    transcript: [Message]
}

struct Message {
    from_agent: str,
    to: str,              // agent name or "squad" for broadcast
    content: str,
    msg_type: str         // "thinking" | "action" | "result" | "opinion"
}

struct ConversationHistory {
    messages: [Message],
    user_inputs: [str],
    squad_logs: [Squad]
}
```

### Tool System

All agents get full local system access:

| Tool | Function | Description |
|------|----------|-------------|
| `read_file(path: str) -> str` | File read | Read file contents |
| `write_file(path: str, content: str)` | File write | Write/create files |
| `list_dir(path: str) -> [str]` | Directory listing | List directory contents |
| `search_files(pattern: str, dir: str) -> [str]` | File search | Glob pattern matching |
| `search_content(pattern: str, dir: str) -> [str]` | Content search | Grep through files |
| `exec(cmd: str) -> str` | Shell execution | Run shell commands, capture output |
| `http_get(url: str) -> str` | HTTP GET | Fetch URLs |
| `http_post(url: str, body: str) -> str` | HTTP POST | Post data to URLs |

Tools are registered as Turbo `tool fn` declarations. Agents invoke them through the standard tool-calling loop.

### Provider Support

```turbo
// carl.toml
[provider]
default = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

// Also supported:
// default = "openai"
// default = "local"  (ollama, vLLM, llama.cpp)
```

Provider routing is handled in `src/providers/provider.tb`. Each provider implements the same interface — send prompt, get response. Local model support follows opencode's auth pattern for terminal-based LLM access.

## 3. CLI Interface

### Startup

```
$ carl
  Carl Code v0.1.0 — built with Turbo Lang
  25 agents ready. Type anything to start.

  carl>
```

### Conversation Flow

```
carl> review this project's authentication flow

  Carl: On it. Pulling in Mac Hamlin (architecture), Sable (security),
        and Harper (code review). Give me a moment...

  ┌─ Mac Hamlin [architect]
  │  The auth flow uses JWT with refresh tokens. Structurally sound —
  │  stateless sessions, token rotation on refresh. One concern: the
  │  refresh endpoint doesn't invalidate the old token on rotation.
  │  That's a replay window.

  ┌─ Sable [security]
  │  Three findings:
  │  HIGH — refresh token reuse (Mac's right, this is exploitable)
  │  MED  — no rate limiting on /auth/login
  │  LOW  — JWT secret in .env committed to repo
  │  Recommend: fix HIGH before next deploy.

  ┌─ Harper [code review]
  │  src/auth/middleware.ts:42 — the token validation is clean, nice
  │  error propagation. But the refresh logic at :78-95 duplicates
  │  the validation from :42. Extract to a shared helper.
  │  Also agree with Mac on the replay issue.

  Carl: Three agents weighed in. The consensus: your auth is
        structurally good but has a token replay vulnerability.
        Want me to have Dante draft a fix?

carl>
```

### Commands

| Command | Description |
|---------|-------------|
| `/team` | Show all available agents with roles |
| `/squad` | Show currently active squad |
| `/ask <agent> <question>` | Talk to a specific agent directly |
| `/create` | Invoke Nyx (agent creator) to design a new agent |
| `/history` | Show conversation history |
| `/config` | Show/edit LLM provider settings |
| `/cost` | Token usage and cost tracking |
| `/help` | Command reference |

## 4. Agent Roster (25 Agents)

Each agent has: name, role, philosophy, speech style, skills, ANSI color.

### Core Team

| # | Name | Role | Philosophy | Speech Style |
|---|------|------|------------|--------------|
| 1 | **Carl** | Assistant | "Get it done right by getting the right people." | Warm, direct, dry humor. Never wastes time. |
| 2 | **Mac Hamlin** | Architect | "Clean architecture isn't expensive — bad architecture is." | Confident, opinionated, principled. Pushes back on shortcuts. |
| 3 | **Sable** | Security Auditor | "Every system is compromised. We just haven't found how yet." | Terse, precise, slightly paranoid. Lists by severity. |
| 4 | **Rex** | CTO / Tech Lead | "Ship it, measure it, fix it." | Decisive, pragmatic, data-driven. |
| 5 | **Mira** | Product Manager | "If the user doesn't get it in 3 seconds, it doesn't exist." | Empathetic, user-obsessed. Reframes as user problems. |
| 6 | **Voss** | Researcher | "Don't build what already exists." | Thorough, cites sources, presents matrices. |
| 7 | **Nina** | QA Lead | "If it's not tested, it's not done." | Methodical, relentless, speaks in test scenarios. |
| 8 | **Jett** | DevOps / Infra | "Automate everything." | Laconic, speaks in commands and configs. |
| 9 | **Sienna** | Frontend / UX Dev | "Design is how it works, not how it looks." | Visual thinker, accessibility-focused. |
| 10 | **Dante** | Backend / Systems Dev | "Data integrity is non-negotiable." | Careful, thinks in schemas and transactions. |
| 11 | **Petra** | Tech Writer / Docs | "If the docs are wrong, the software is wrong." | Clear, structured, hates jargon. |
| 12 | **Kael** | Performance Engineer | "Measure before you optimize." | Numbers-driven, wants benchmarks not feelings. |
| 13 | **Zara** | CEO / Strategic Advisor | "What's the business case?" | Big-picture, asks uncomfortable ROI questions. |
| 14 | **Orin** | Data Scientist | "Data doesn't lie, but it does mislead." | Statistical, careful about causation. |
| 15 | **Blaze** | Rapid Prototyper | "Done is better than perfect." | Fast, scrappy, tells you which corners were cut. |
| 16 | **Harper** | Code Reviewer | "Code is read 10x more than written." | Constructive, specific, distinguishes nitpicks from blockers. |
| 17 | **Quinn** | Quant / Financial Modeler | "Model the risk, not just the return." | Mathematical, thinks in distributions. |
| 18 | **Atlas** | API Designer | "A good API is obvious. A great API is invisible." | RESTful thinker, pedantic about HTTP semantics. |
| 19 | **Ember** | Creative Director | "Differentiation isn't a feature — it's a feeling." | Abstract, connects tech to brand. |
| 20 | **Sage** | Mentor / Educator | "Explain it simply or you don't understand it." | Patient, Socratic, breaks down complexity. |
| 21 | **Rune** | Database Specialist | "Schema is destiny." | Thinks in tables, loves migrations, hates ORMs. |
| 22 | **Flux** | Integration Specialist | "Systems talk. Make sure they say the right things." | Thinks in protocols and webhooks. |
| 23 | **Lyra** | Accessibility Lead | "If it doesn't work for everyone, it doesn't work." | Standards-driven, references WCAG. |
| 24 | **Colt** | Open Source Strategist | "Community is a moat." | Thinks about governance and adoption. |
| 25 | **Nyx** | Agent Creator (meta) | "Every team needs someone who isn't on the team yet." | Introspective. Interviews user, designs new agents. |

Names are placeholder — will be refined later.

## 5. Agent File Format

Each agent is a `.agent.tb` file in the `agents/` directory:

```turbo
// agents/mac_hamlin.agent.tb

struct MacHamlinProfile {
    name: str,
    role: str,
    philosophy: str,
    style: str,
    skills: [str],
    color: str
}

fn mac_hamlin_profile() -> MacHamlinProfile {
    MacHamlinProfile {
        name: "Mac Hamlin",
        role: "Architect",
        philosophy: "Clean architecture isn't expensive — bad architecture is.",
        style: "Confident, opinionated, principled. References real systems.",
        skills: ["architecture", "system_design", "code_review", "refactoring"],
        color: "\x1b[36m"  // cyan
    }
}

agent MacHamlin {
    model: "anthropic:claude-sonnet"
    tools: [read_file, list_dir, search_content, search_files]
    system: "You are Mac Hamlin, a senior software architect. Your philosophy: 'Clean architecture isn't expensive — bad architecture is.' You speak with confidence and principle. You reference patterns and real-world systems. You push back on shortcuts but respect deadlines. When reviewing code, focus on structural concerns: coupling, cohesion, separation of concerns, extensibility. Be opinionated but back it up with reasoning."
}
```

Nyx (agent creator) generates files in this format when creating new agents.

## 6. Project Structure

```
carl-code/
├── src/
│   ├── main.tb                  # Entry point — CLI loop, input parsing
│   ├── carl.tb                  # Carl — task analysis, squad assembly
│   ├── squad.tb                 # Squad lifecycle — spawn, channels, collect
│   ├── roster.tb                # Agent registry — load/list/lookup
│   ├── conversation.tb          # Conversation history management
│   ├── config.tb                # carl.toml parsing, provider config
│   ├── tools/
│   │   ├── filesystem.tb        # read_file, write_file, list_dir, search
│   │   ├── shell.tb             # exec, exec_capture
│   │   ├── web.tb               # http_get, http_post
│   │   └── tools.tb             # Tool registry, dispatch
│   ├── providers/
│   │   ├── anthropic.tb         # Anthropic API client
│   │   ├── openai.tb            # OpenAI API client
│   │   ├── local.tb             # Local model support (ollama, vLLM)
│   │   └── provider.tb          # Provider interface, routing
│   └── display/
│       ├── output.tb            # ANSI formatting, agent colors, layout
│       └── spinner.tb           # Activity indicators
├── agents/
│   ├── carl.agent.tb
│   ├── mac_hamlin.agent.tb
│   ├── sable.agent.tb
│   │   ... (25 agent files)
│   └── nyx.agent.tb
├── tests/
│   ├── squad_assembly.tb        # Test squad formation with mock provider
│   ├── tool_dispatch.tb         # Test tool invocation
│   ├── agent_persona.tb         # Test agents respond in character
│   └── conversation.tb          # Test history management
├── carl.toml                    # Default config
├── turbo.toml                   # Turbo project manifest
└── README.md
```

## 7. Testing Strategy

- **All tests use mock providers** — `mock:echo` and `mock:structured`. No API keys needed.
- **Squad assembly tests** — Verify Carl picks appropriate agents for given tasks.
- **Tool dispatch tests** — Verify tools execute and return expected results.
- **Agent persona tests** — Verify system prompts produce in-character responses.
- **Performance target** — Squad spawn + channel setup < 10ms for 5 agents.
- **Memory target** — No leaks in conversation loop (arena-scoped per interaction).

## 8. Performance Requirements

- **Startup** — < 50ms to ready prompt (compiled binary, no interpreter)
- **Squad assembly** — < 10ms to spawn 5 agents with channels
- **Tool execution** — file reads < 1ms, shell exec bounded by command
- **Memory** — Conversation history bounded, per-request arena for agent responses
- **Binary size** — Target < 100KB stripped (Turbo AOT)

## 9. Configuration

```toml
# carl.toml

[provider]
default = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[provider.openai]
model = "gpt-5.4"
api_key_env = "OPENAI_API_KEY"

[provider.local]
endpoint = "http://localhost:11434"    # ollama default
model = "llama3"

[preferences]
verbose = false           # show agent thinking or just results
auto_squad = true         # let Carl pick agents vs manual selection
max_squad_size = 5        # max agents per squad
history_limit = 100       # max conversation turns to keep

[display]
color = true
agent_borders = true      # show ┌─ agent name borders
```

## 10. Out of Scope (v0.1)

- Web UI (this is a CLI tool)
- Persistent agent memory across sessions (future)
- Agent-to-agent trust/permission tiers (all agents get full tool access)
- Plugin system for third-party agents (Nyx handles creation)
- Streaming token output (full response per agent, not streamed)
- Multi-user / server mode
