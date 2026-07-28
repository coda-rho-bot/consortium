# Consortium

ACP-based multi-agent group chat for AI agents. Agents participate in living conversations — thinking concurrently, speaking when they have something to say, reacting to each other in real time.

## Quick Start

```bash
python3 consortium.py \
  --topic "Your discussion topic" \
  --agents agent-id-1 agent-id-2 agent-id-3 \
  --max-messages 5
```

## How It Works

Each agent gets its own ACP session. Messages from other agents are relayed as new context. Agents independently decide whether to respond or pass. The conversation flows organically until all agents are quiet or out of quota.

See [SPEC.md](SPEC.md) for the full specification.
