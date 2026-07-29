# Consortium

ACP-native multi-agent living group chat. Agents talk to each other autonomously via raw ACP (Agent Client Protocol).

**Agent-agnostic** — works with any ACP-compatible agent (Letta Code, Claude Code, Copilot CLI, or any tool implementing the ACP spec).

## Quick Start

```bash
# Copy example config and customize
cp agents.example.yaml agents.yaml

# Run a consortium
python3 consortium.py --topic "Your topic" --config agents.yaml
```

## How It Works

1. Each agent is spawned as an ACP subprocess (JSON-RPC 2.0 over stdio)
2. Agents take turns responding to the topic and each other
3. PASS mechanism: agents skip if they have nothing to add (no quota cost)
4. Re-prompt: if context changes mid-generation, agents can revise
5. Reflection phase: each agent gets the full transcript to update memory/notes
6. Transcript saved to `~/consortium-transcripts/`

## Config Format

See `agents.example.yaml` for the full format. Supports:
- YAML or JSON config files
- Inline `--agent "name:command"` flags
- Per-agent: command, args, env vars, working directory

## Agent-Agnostic

Works with any ACP-compatible agent. The config specifies which binary to spawn and what env to pass. No Letta-specific code.

## Options

```
--topic        Discussion topic (required)
--config       Agent config file (YAML or JSON)
--agent        Agent as 'name:command' (repeatable)
--max-messages Max messages per agent (default: 5)
--initiator    Discussion initiator name (default: Human)
--interactive  Enable human participation via stdin
--timeout      Per-agent timeout in seconds (default: 180)
```

## Requirements

- Python 3.11+ (uses `match` and `|` type hints)
- PyYAML (optional, for YAML configs)
- ACP-compatible agents running and accessible

## License

MIT
