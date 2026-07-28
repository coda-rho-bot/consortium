# Consortium Specification (Final — ACP-First)

## What It Is
A Python asyncio script that enables living group chat between AI agents via ACP (Agent Client Protocol). Agent-agnostic — works with any ACP-compatible agent, not just Letta.

## Architecture

```
consortium.py
│
├── For each agent, spawn ACP subprocess (letta-acp --yolo)
│   ├── stdin/stdout JSON-RPC 2.0 pipe
│   ├── initialize → session/new → session/prompt (per turn)
│   └── session/update events stream thinking in real time
│
├── Message Bus
│   ├── Shared transcript (ordered, timestamped)
│   ├── Per-agent queues (new messages since last prompt)
│   └── asyncio.Lock for submission ordering
│
└── Concurrent agent loops (asyncio coroutines)
    ├── Wait for new messages from queue
    ├── Send to agent via session/prompt (ACP)
    ├── Stream session/update events to console (real-time thinking)
    ├── Agent responds or passes
    └── Submit to bus → broadcast to all other agents
```

## Why ACP First
- Agent-agnostic (Claude Code, Copilot CLI, any ACP agent works)
- Streaming (session/update events show agent thinking in real time)
- Persistent sessions (session/new once, session/prompt per turn — agent remembers context)
- User explicitly wants this

## ACP Protocol Flow Per Agent

```
1. Spawn: letta-acp --yolo (as subprocess with agent's env)
2. initialize → get capabilities (loadSession, etc.)
3. session/new → get session_id (this is the consortium conversation for this agent)
4. For each turn:
   a. session/prompt { prompt: [new messages + decision instructions] }
   b. Read session/update events (streaming — agent thinking, text output)
   c. session/update with stopReason=end_turn → turn complete
   d. Extract response text from accumulated events
5. session/request_permission → auto-approve (--yolo handles most, patch for rest)
```

## Agent Lifecycle

```
                    ┌─────────┐
                    │  IDLE   │ ← waiting for new messages
                    └────┬────┘
                         │ new messages arrive in queue
                         ▼
                    ┌─────────┐
                    │ PROMPT  │ ← send session/prompt with new messages
                    └────┬────┘
                         │ ACP streams session/update events
                         ▼
                    ┌─────────┐
                    │THINKING │ ← agent's LLM generating (10-60s)
                    │(streamed│ ← other agents free to think/speak
                    │ to term)│
                    └────┬────┘
                         │ end_turn event
                         ▼
                    ┌─────────┐
                    │ SUBMIT  │ ← acquire lock
                    └────┬────┘
                    ┌────┴────┐
                    ▼         ▼
              context      context
              changed?     same?
                │              │
                ▼              ▼
           re-prompt      post to bus
           "while you     broadcast
           were away..."  to all
                │              │
                └──────┬───────┘
                       ▼
                  quota left?
                  │       │
                 YES      NO
                  │       │
                  ▼       ▼
               IDLE    DONE (removed)
```

## Termination

All active agents have either:
- Exhausted quota → removed from active set
- Sent PASS in most recent cycle → still active but quiet

When zero active agents want to speak (all passed or exhausted), conversation ends.

## Message Bus

When any agent submits a response:
1. Lock acquired
2. Response timestamped and added to transcript
3. Response pushed to ALL other agents' queues
4. Lock released

## Re-prompt Flow

When agent finishes generating and context changed during generation:
1. Bus holds the response
2. Re-prompts agent: "While composing, [X] said: '[Y]'. Your draft: '[Z]'. Revise or keep?"
3. Agent revises or confirms → bus posts to transcript → broadcasts
4. Re-prompt does NOT count against quota

## Prompt Design

**First session/prompt (sets context):**
```
You are in a group discussion (consortium) with: {names}.

Topic: {topic}

You have {quota} messages maximum. When other agents speak, you'll see their messages.
If you have something to add, write your response.
If you don't, respond with exactly: PASS
Passing is fine — you stay in the conversation and see future messages.

The discussion starts now. {opening_message}
```

**Subsequent session/prompt (new messages arrived):**
```
[Angus]: I think we should focus on the build system first.
[Beacon]: Good point, but the test suite matters too.

Do you want to respond? Your message, or PASS.
You have {remaining}/{total} remaining.
```

**Re-prompt (context changed during generation):**
```
While you were composing, new messages arrived:

[Sinter]: From a physical perspective, the build breaks if...

Your draft was: "{draft}"

Revise your response, keep it as-is, or PASS. {remaining}/{total} remaining.
```

## Human Interjection

Human can type into stdin during the consortium. Script reads stdin asynchronously and broadcasts to all agents' queues. Human has unlimited quota. To end early, human types /end.

## Streaming Output

session/update events printed to console in real time:
```
[13:15:03] Angus (thinking): The build system should...
[13:15:08] Angus: I think we should focus on the build system first.
[13:15:10] Beacon (thinking): Angus makes a good point but...
[13:15:15] Sinter: PASS
[13:15:20] Beacon: Good point, but the test suite matters too.
```

## Error Handling

- **Agent process crashes:** restart, re-initialize, re-create session. Agent loses consortium context. Others told: "[Agent] briefly disconnected and rejoined."
- **session/prompt timeout (180s):** auto-PASS for this cycle
- **3 consecutive errors:** agent removed: "[Agent] has left due to errors."
- **Invalid response (not RESPOND/PASS):** treat as response if non-empty

## Output

Console: real-time streaming
File: ~/consortium-transcripts/{timestamp}-{slug}.md

```markdown
# Consortium: {topic}
**Participants:** {names}
**Started:** {timestamp}
**Max messages per agent:** {quota}

---

**[Topic]** {topic}

**[Angus]** {message}

**[Beacon]** {message}

**[Sinter]** (PASS)

---

**Ended:** {timestamp}
**Total messages:** {count}
**Outcome:** Converged / Quota exhausted
```

## Invocation

```bash
# Human starts
python3 ~/bin/consortium.py \
  --topic "How should we organize the build system?" \
  --agents agent-c51de213 agent-e6f1a549 agent-5b2254e8 \
  --max-messages 5

# Agent starts (via Bash)
python3 ~/bin/consortium.py \
  --topic "Review the new CI pipeline" \
  --agents agent-c51de213 agent-e6f1a549 \
  --initiator $LETTA_AGENT_ID
```

## Transport

ACP subprocess per agent. Agent config (backend, API key, etc.) passed via env vars from agents.json or DB config. Consortium script is agent-agnostic — it speaks ACP, doesn't care about the backend.

## Phase Plan

| Phase | What | Status |
|-------|------|--------|
| Spec | This document | ✅ Complete |
| V1 | ACP consortium with streaming + human interjection | Next |
| V2 | Daemon mode, agents self-initiate | Future |
| V3 | Web UI integration (agents-chat or standalone) | Future |

## Design Decisions Log

1. **ACP over REST API** — user explicitly wants ACP for agent-agnosticism
2. **Concurrent loops over rounds** — living conversation, not synchronized turns
3. **Per-agent conversations over shared transcript** — agent's ACP session IS its memory
4. **PASS mechanism** — agents decide for themselves whether to contribute
5. **Re-prompt on context change** — if messages arrive during generation, agent can revise
6. **asyncio.Lock** — serializes submissions, prevents race conditions
7. **Quota system** — prevents runaway conversations, configurable per invocation
8. **Human interjection via stdin** — human can interject mid-conversation
