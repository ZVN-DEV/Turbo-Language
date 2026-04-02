# First-Class Agentic AI Primitives

## Why This Matters
- AI agents are the fastest-growing software category
- Every language retrofits agent support via libraries (LangChain, Semantic Kernel, Koog, CrewAI, etc.)
- No language has built-in primitives for AI agents — we'd be first
- A language that natively understands agents can: optimize tool schemas at compile time, validate agent configurations statically, provide rich IDE support (autocomplete tool names, type-check structured outputs against schemas)

## Design Decision
`agent` and `tool` are **first-class language keywords** like `fn` or `struct`. The compiler understands agents natively.

### Actors vs Agents

Turbo has both `actor` (defined in the concurrency model) and `agent` (defined here). These serve fundamentally different purposes and should not be confused:

- **An `actor` manages concurrent state.** Actors are a concurrency primitive for isolated stateful processes, inspired by Erlang/Elixir. They communicate via message passing, can be organized into supervision trees for fault tolerance, and have no knowledge of AI or LLMs. Use actors for things like connection pools, rate limiters, caches, and any stateful service that needs isolation and fault recovery. Actors are a general-purpose concurrency tool.

- **An `agent` manages AI behavior.** Agents are an AI primitive that wraps an LLM with tools, memory, and streaming capabilities. They define a model, a set of tools, a memory strategy, and custom routing/processing logic. Agents are specifically for building LLM-powered applications.

**How they compose:** Agents use actors under the hood for supervision. When you configure an agent with restart strategies, circuit breakers, or rate limiting, the runtime creates actor-based supervision trees to manage the agent's lifecycle. But the developer interacts with the `agent` keyword, not with raw actors. The relationship is compositional, not substitutional:

```
// An actor: concurrency primitive, no AI involved
actor ConnectionPool {
  state: [Connection]
  fn acquire(self) -> Connection { ... }
  fn release(self, conn: Connection) { ... }
}

// An agent: AI primitive, may use actors internally for supervision
agent Assistant {
  model: "claude-sonnet"
  tools: [search, summarize]
  // The runtime supervises this agent using actors,
  // but the developer never touches the actor layer.
}
```

**Rule of thumb:** If it talks to an LLM, it is an `agent`. If it manages concurrent state without AI, it is an `actor`. If you need both (e.g., an AI agent that also manages a connection pool), compose them: the agent uses actors as dependencies, not as a base class.

## Getting Started

> **Coming from JavaScript?** If you have used the OpenAI or Anthropic SDKs, Vercel AI SDK, or LangChain in JS/TS, you already know the concepts. Turbo just makes them first-class:
> - `tool fn` = a function the LLM can call (like OpenAI function calling, but type-safe at compile time)
> - `agent` = a configured LLM with tools and memory (like a LangChain Agent, but a language keyword)
> - `agent.stream()` = streaming responses (like `for await (const chunk of stream)` in JS)
> - `agent.ask()` = single response (like `await openai.chat.completions.create()`)

### Your First Agent in 5 Lines

A JS developer should be able to write their first agent in under 5 lines. Here it is:

```
// 5 lines: import, define a tool, create an agent, ask, print
tool fn search(query: str) -> [str] { /// Search the web; await search_api.query(query) }
let agent = Agent.new("claude-sonnet", tools: [search])
let answer = await agent.ask("What's the latest news about AI?")
print(answer)
```

That's it. No framework, no configuration file, no boilerplate. The compiler generates the JSON Schema for your tool, validates everything at compile time, and handles streaming/retry/serialization for you.

### Quick Start

You don't need to understand the full agent system to get started. The simplest agent is just a few lines — like calling an API.

```
// Simplest possible agent — one line, like calling an API
// JS equivalent: const response = await anthropic.messages.create({model: "...", messages: [...]})
let response = await Agent.quick("claude-sonnet", "What is 2 + 2?")
print(response)  // "4"
```

```
// With tools — still simple
// JS equivalent: defining a function for OpenAI function calling, but without manual JSON Schema
tool fn get_weather(city: str) -> WeatherData {
  /// Get current weather for a city
  await weather_api.fetch(city)
}

let agent = Agent.new("claude-sonnet", tools: [get_weather])
let answer = await agent.ask("What's the weather in Tokyo?")
print(answer)  // "It's currently 22C and sunny in Tokyo."
```

```
// With conversation history — add memory in one line
// JS equivalent: maintaining a messages[] array manually. Turbo does it for you.
let agent = Agent.new("claude-sonnet",
  tools: [get_weather, search_web],
  memory: ConversationMemory(max_turns: 20)
)

let a1 = await agent.ask("What's the weather in Tokyo?")
let a2 = await agent.ask("How about Osaka?")  // remembers we're talking about weather
```

### Streaming Made Simple

Stream responses token-by-token, just like using `EventSource` or `ReadableStream` in JavaScript.

```
// Stream responses — like EventSource in JavaScript
for await token in agent.stream("Tell me a story") {
  print(token, end: "")
}

// Or collect the full response when you don't need streaming
let full = await agent.stream("Tell me a story").collect()

// Stream with rich token metadata
for await token in agent.stream("What's the weather?") {
  match token.kind {
    .text(t) => print(t, end: "")
    .tool_call(name, args) => print("\n[Calling {name}...]")
    .tool_result(result) => print("\n[Got result]")
    .done(usage) => print("\n\nTokens used: {usage.total}")
  }
}
```

### Agent as a Service

Expose an agent as an HTTP endpoint with minimal boilerplate — go from prototype to production in seconds.

```
// Expose an agent as an HTTP endpoint — one line
let agent = Agent.new("claude-sonnet", tools: [get_weather])
agent.serve(port: 3000)
// POST /ask  { "message": "What's the weather?" }
// POST /stream  { "message": "Tell me a story" }  (SSE)
```

```
// Or integrate with a web framework for full control
let app = Server.new()

app.post("/chat", async (req) => {
  let input = req.json<ChatInput>()?
  let response = await agent.ask(input.message)
  Response.json({ message: response })
})

app.post("/stream", async (req) => {
  let input = req.json<ChatInput>()?
  Response.sse(agent.stream(input.message))
})

await app.listen(3000)
```

```
// WebSocket support for real-time chat
app.ws("/ws", async (socket) => {
  let agent = Agent.new("claude-sonnet",
    memory: ConversationMemory(max_turns: 50)
  )

  for await msg in socket.messages() {
    for await token in agent.stream(msg.text) {
      await socket.send(token.text)
    }
  }
})
```

## The Seven Primitives

### 1. `tool` Keyword — Declare LLM-Callable Functions

> **JS equivalent:** In JavaScript, defining a tool for OpenAI/Anthropic function calling requires writing a JSON Schema by hand (or using Zod + a converter). In Turbo, you just write a function with `tool` in front of it -- the compiler generates the JSON Schema automatically from the types and doc comments.

```
tool fn get_weather(city: str, units: TemperatureUnit = .celsius) -> WeatherData {
  /// Get the current weather for a city
  /// @param city - The city name (e.g., "Tokyo", "New York")
  /// @param units - Temperature units (celsius or fahrenheit)
  await weather_api.fetch(city, units)
}

// Compare with the JS/TS equivalent (Vercel AI SDK):
// const weatherTool = tool({
//   description: 'Get the current weather for a city',
//   parameters: z.object({
//     city: z.string().describe('The city name'),
//     units: z.enum(['celsius', 'fahrenheit']).default('celsius'),
//   }),
//   execute: async ({ city, units }) => { ... }
// })
// Turbo: just write the function. The compiler does the rest.
```

What the compiler does:
- Auto-generates JSON Schema from the function signature + doc comments
- Type-checks that the return type is serializable
- Validates default values at compile time
- Makes the schema available at compile time for agent validation
- Generates TypeScript-compatible type definitions for the tool

Generated JSON Schema example:

```json
{
  "name": "get_weather",
  "description": "Get the current weather for a city",
  "input_schema": {
    "type": "object",
    "properties": {
      "city": {
        "type": "string",
        "description": "The city name (e.g., \"Tokyo\", \"New York\")"
      },
      "units": {
        "type": "string",
        "enum": ["celsius", "fahrenheit"],
        "description": "Temperature units (celsius or fahrenheit)",
        "default": "celsius"
      }
    },
    "required": ["city"]
  }
}
```

### 2. `agent` Keyword — Declare an Agent

```
agent Assistant {
  model: "claude-sonnet"
  system: "You are a helpful assistant."
  tools: [get_weather, search_web, run_code]
  memory: ConversationMemory(max_turns: 50)
  max_tokens: 4096
  temperature: 0.7

  // Custom routing logic
  fn route(self, input: str) -> AgentAction {
    match classify(input) {
      .weather => .use_tool(get_weather)
      .code => .use_tool(run_code)
      _ => .respond_directly
    }
  }

  // Custom pre/post processing
  fn pre_process(self, input: str) -> str {
    sanitize(input)
  }

  fn post_process(self, output: Response) -> Response {
    output |> add_citations |> format_markdown
  }
}
```

What the compiler does:
- Validates tool references exist and are actually `tool` functions
- Type-checks that tools' return types are serializable
- Validates model string against known providers (with escape hatch for custom)
- Checks memory configuration is valid
- Provides IDE autocomplete for all agent configuration fields

### 3. `Stream<Token>` Type — First-Class Streaming

```
// Agent streaming
let agent = Assistant.new()
for await token in agent.stream("What's the weather?") {
  print(token.text)

  // Rich token metadata
  match token.kind {
    .text(t) => display(t)
    .tool_call(name, args) => show_tool_use(name, args)
    .tool_result(result) => show_result(result)
    .thinking(thought) => show_thinking(thought)
    .done(usage) => show_stats(usage)
  }
}

// Stream composition (arrow syntax preferred)
let processed = agent.stream(input)
  |> filter_stream((t) => t.kind != .thinking)
  |> map_stream((t) => t.text)
  |> buffer_stream(10)
```

### 4. Structured Output — Parse LLM Responses into Types

> **JS equivalent:** Like using Zod schemas with `generateObject()` in Vercel AI SDK, or `response_format: { type: "json_schema" }` in OpenAI. But in Turbo, the schema is generated from a regular struct -- no Zod, no manual schema definition.

```
@derive(Schema)
struct MovieReview {
  title: str
  rating: f64      // 0.0 to 10.0
  summary: str
  pros: [str]
  cons: [str]
  recommended: bool
}

// The compiler generates a JSON Schema from MovieReview
// and validates the LLM response matches at runtime
let review = await agent.structured<MovieReview>(
  "Review the movie Inception"
)
// review is guaranteed to be a valid MovieReview

// With validation
@derive(Schema)
@schema(validate)
struct UserProfile {
  name: str                          // required
  age: u32 { range: 0..150 }        // with constraint
  email: str { pattern: EMAIL_REGEX } // with pattern
}
```

### 5. Supervision — Elixir-Inspired Agent Reliability

```
// Supervisor for agents
let supervisor = AgentSupervisor.new(strategy: .one_for_one) {
  // Restart individual agents on failure
  .child(Assistant.new(), restart: .permanent)
  .child(CodeAgent.new(), restart: .transient)  // only restart on abnormal exit
  .child(MonitorAgent.new(), restart: .temporary) // never restart
}

// Circuit breaker for API calls
agent ResilientAssistant {
  model: "claude-sonnet"

  @circuit_breaker(
    failure_threshold: 5,
    reset_timeout: 30.seconds(),
    fallback: self.cached_response
  )
  fn handle(self, input: str) -> Stream<Response> {
    // If this fails 5 times, circuit opens and uses fallback
    await self.complete(input)
  }
}

// Retry with backoff
agent RetryAgent {
  @retry(
    max_attempts: 3,
    backoff: exponential(base: 1.second(), max: 30.seconds()),
    retry_on: [RateLimitError, TimeoutError]
  )
  fn complete(self, input: str) -> Response ! Error {
    await self.model.complete(input)
  }
}
```

### 6. Memory — Built-in Agent Memory Abstractions

```
// Short-term (conversation) memory
let memory = ConversationMemory(max_turns: 50)

// Long-term (vector) memory
let long_memory = VectorMemory(
  backend: VectorStore.connect("default"),  // configurable backend
  collection: "knowledge_base"
)

// Composite memory
let memory = CompositeMemory {
  short_term: ConversationMemory(max_turns: 20)
  long_term: VectorMemory { ... }
  strategy: .short_term_first  // check short-term, then long-term
}

agent MemoryAgent {
  memory: CompositeMemory { ... }

  fn handle(self, input: str) -> Stream<Response> {
    // Memory is automatically queried and injected into context
    let context = await self.memory.recall(input)
    let response = await self.complete(input, context: context)

    // Automatically store important exchanges
    await self.memory.store(input, response)

    yield response
  }
}
```

### 7. Tracing — Built-in Observability

```
// All agent executions are automatically traced
agent TracedAgent {
  tracing: .enabled  // or .disabled, .sampling(rate: 0.1)

  fn handle(self, input: str) -> Stream<Response> {
    // Automatic trace events:
    // - agent.think: planning/reasoning
    // - agent.tool_call: tool invocations
    // - agent.tool_result: tool results
    // - agent.response: final response
    // - agent.error: errors
    // - agent.token_usage: token counts

    yield await self.complete(input)
  }
}

// Custom trace spans
fn complex_pipeline(input: str) -> Output ! Error {
  trace("preprocessing") {
    let clean = sanitize(input)
  }
  trace("inference") {
    let result = await agent.complete(clean)
  }
  trace("postprocessing") {
    transform(result)
  }
}

// Export to observability backends
TracingConfig {
  exporters: [
    ConsoleExporter.new(),
    OTLPExporter.new(endpoint: "localhost:4317"),
    LangSmithExporter.new(api_key: env("LANGSMITH_KEY")),
  ]
}
```

## Multi-Agent Orchestration

> **JS equivalent:** Like CrewAI or LangGraph in Python/JS, but built into the language. No framework dependency, no YAML configuration -- just compose agents with language constructs.

```
// Agent team with coordinator
let team = AgentTeam {
  coordinator: PlannerAgent.new()
  workers: [
    ResearchAgent.new(),
    CodeAgent.new(),
    ReviewAgent.new()
  ]
  strategy: .plan_and_execute
}

let result = await team.run("Build a REST API for a todo app")

// Pipeline pattern
let pipeline = AgentPipeline {
  stages: [
    DraftAgent.new(),
    EditAgent.new(),
    FactCheckAgent.new(),
    FormatAgent.new(),
  ]
}

// Debate/consensus pattern
let debate = AgentDebate {
  agents: [OptimistAgent.new(), CriticAgent.new(), SynthesizerAgent.new()]
  rounds: 3
  consensus_threshold: 0.8
}
```

## Testing Agents

> **JS equivalent:** Like using Jest mocks for API calls, but purpose-built for LLM agents. `MockModel` is like `jest.fn()` for the LLM -- you control exactly what it returns, making tests deterministic and free (no API calls). `mock(tool)` is like `jest.spyOn(module, 'function')`.

Agents are testable from day one. Mock models, mock tools, snapshot agent outputs -- all with the standard `turbo/test` framework. No special test harness needed.

### Mocking the LLM

Use `MockModel` to control exactly what the model returns. Tests are deterministic, fast, and free (no API calls).

```
import { mock, MockModel } from "turbo/test"

@test
fn test_weather_agent() {
  // Mock the LLM with scripted responses
  let mock_model = MockModel.new(responses: [
    "I'll check the weather for you.",
    tool_call("get_weather", { city: "Tokyo" })
  ])

  // Mock the tool
  let mock_weather = mock(get_weather, returns: WeatherData {
    temp: 22.0, condition: "Sunny"
  })

  let agent = WeatherAgent.new(model: mock_model, tools: [mock_weather])
  let response = await agent.ask("What's the weather in Tokyo?")

  assert(response.contains("Sunny"))
  assert_eq(mock_weather.call_count(), 1)
}
```

### Snapshot Testing for Agent Responses

Capture structured agent output and compare against saved snapshots. When agent behavior changes intentionally, run `turbolang test --update-snapshots` to accept the new output.

```
@test
fn test_agent_structured_output() {
  let agent = AnalysisAgent.new(model: MockModel.deterministic())
  let result = await agent.analyze("quarterly report data")
  assert_snapshot(result)  // Compares against saved snapshot
}
```

### Testing Multi-Step Tool Use

Verify that agents call the right tools in the right order with the right arguments.

```
@test
fn test_multi_tool_agent() {
  let mock_model = MockModel.new(responses: [
    tool_call("search_db", { query: "active users" }),
    tool_call("format_report", { data: "{{search_result}}" }),
    "Here is your report on active users."
  ])

  let mock_search = mock(search_db, returns: [
    { name: "Alice", active: true },
    { name: "Bob", active: true },
  ])
  let mock_format = mock(format_report, returns: "Formatted report...")

  let agent = ReportAgent.new(model: mock_model, tools: [mock_search, mock_format])
  let response = await agent.ask("Generate a report of active users")

  // Verify tool call order and arguments
  assert_eq(mock_search.call_count(), 1)
  assert_eq(mock_search.last_args().query, "active users")
  assert_eq(mock_format.call_count(), 1)
}
```

### Testing Streaming Responses

```
@test
fn test_streaming_output() {
  let mock_model = MockModel.new(responses: ["Hello, world!"])
  let agent = Agent.new(model: mock_model)

  let tokens = await agent.stream("Say hello").collect()

  assert(tokens.len() > 0)
  let full_text = tokens.map((t) => t.text).join("")
  assert_eq(full_text, "Hello, world!")
}
```

### Testing Error Recovery and Supervision

```
@test
fn test_agent_retries_on_failure() {
  // First call fails, second succeeds
  let mock_model = MockModel.new(responses: [
    err(RateLimitError { retry_after: 1.second() }),
    "Success after retry"
  ])

  let agent = RetryAgent.new(model: mock_model)
  let response = await agent.ask("test query")

  assert_eq(response, "Success after retry")
  assert_eq(mock_model.call_count(), 2)
}

@test
fn test_circuit_breaker_opens() {
  // All calls fail
  let mock_model = MockModel.new(responses: [
    err(NetworkError.ConnectionRefused("api.example.com", 443)),
  ], repeat: true)

  let agent = ResilientAssistant.new(model: mock_model)

  // Trigger circuit breaker (5 failures)
  for _ in 0..5 {
    let _ = await agent.ask("test")
  }

  // Next call should hit the fallback, not the model
  let response = await agent.ask("test")
  assert(response.is_cached())
}
```

## Model Provider Abstraction

> **JS equivalent:** Like the provider pattern in Vercel AI SDK (`import { anthropic } from '@ai-sdk/anthropic'`) but built into the language config. `ModelConfig` is a standard struct that centralizes provider setup -- replacing the scattered API key management and provider initialization you do in JS.

```
// Built-in provider support — configured via a standard struct
let config = ModelConfig {
  providers: {
    "anthropic": ProviderConfig { api_key: env("ANTHROPIC_API_KEY") },
    "openai": ProviderConfig { api_key: env("OPENAI_API_KEY") },
    "local": ProviderConfig { endpoint: "http://localhost:11434" },  // Ollama etc.
  },
  default_model: "claude-sonnet",
  fallback: ["claude-sonnet", "gpt-4o", "local/llama3"],
}

// Register the config (typically in main or app setup)
ModelRegistry.configure(config)

// Custom provider (trait-based)
trait ModelProvider {
  async fn complete(self, request: CompletionRequest) -> Stream<Token>
  async fn embed(self, text: str) -> [f64]
}
```

## Integration With Language Features

| Feature | How It Integrates With Agents |
|---------|------------------------------|
| Type system | Tool schemas generated from types; structured output validated against types |
| Async/streaming | Agent responses are native async streams |
| Pattern matching | Match on token types, agent actions, tool results |
| Error handling | `T ! E` for all agent operations; ? propagation |
| Supervision | Agents are supervised by actor-based supervision trees for fault tolerance (restart, circuit breaking). Actors handle the concurrency; agents handle the AI. |
| Ownership | Agent state is owned; tools can borrow data safely |
| Compile-time | Tool schemas computed at compile time; agent config validated statically |

## Comparison: Native vs Library Approach

| Aspect | Turbo Native Approach | Library Approach (LangChain etc.) |
|--------|--------------------|------------------------------------|
| Tool schema generation | Compile-time from types | Runtime reflection or manual JSON |
| Type safety | Full — compiler validates everything | Partial — runtime errors common |
| IDE support | Autocomplete, inline errors, hover docs | Limited to library API |
| Performance | Compiler optimizes streaming, memory | Library overhead, allocations |
| Error messages | "Tool get_weather expects str, got i32" | "TypeError at runtime" |
| Discovery | `tool` keyword shows in code search | Functions decorated with strings |
