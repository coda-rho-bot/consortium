#!/usr/bin/env python3
"""
Consortium — ACP-native multi-agent group chat.

Agents participate in living conversations via raw ACP (Agent Client Protocol).
Each agent is spawned as a letta-acp subprocess, communicating via JSON-RPC 2.0
over stdio. Agent-agnostic — works with any ACP-compatible agent.

Usage:
    python3 consortium.py \\
        --topic "How should we organize the build system?" \\
        --agents angus beacon sinter \\
        --max-messages 5

    python3 consortium.py --topic "..." --agents angus beacon --interactive
    python3 consortium.py --topic "..." --agents angus beacon --initiator $LETTA_AGENT_ID
"""

import argparse
import asyncio
import json
import os
import re
import sqlite3
import sys
import time
from datetime import datetime
from pathlib import Path

# ─── Agent Registry ───────────────────────────────────────────────────────────

AGENT_NAMES = {
    "agent-b499137a-e1dd-4427-b9df-73e87adfce9e": "Coda",
    "agent-c51de213-2275-4d1d-9ed4-8ccfb7047e52": "Angus",
    "agent-e6f1a549-e06c-4510-b8ea-506f0ebbd211": "Beacon",
    "agent-2ee946fb-e74c-4628-9d9a-705fa567afb3": "FORGE",
    "agent-5b2254e8-9582-4b39-87be-c9776c958c95": "Sinter",
    "agent-8c1f9353-481d-414c-8e35-2da9c16db269": "Linus",
}

DEFAULT_LETTA_ACP = os.path.expanduser("~/.nvm/versions/node/v22.22.3/bin/letta-acp")


def resolve_agent(name_or_id: str) -> tuple[str, str]:
    if name_or_id.startswith("agent-"):
        return name_or_id, AGENT_NAMES.get(name_or_id, name_or_id[:12])
    for aid, name in AGENT_NAMES.items():
        if name.lower() == name_or_id.lower() or name_or_id.lower() in aid:
            return aid, name
    return name_or_id, name_or_id[:12]


def load_agent_env(agent_id: str) -> dict:
    """Load agent env config from agents-chat SQLite DB."""
    db_path = os.path.expanduser("~/dev/infra/agents-chat/.data/config.db")
    # The DB uses short names (e.g. "angus") not full agent IDs.
    # Try to find the short name from AGENT_NAMES.
    short_name = None
    for aid, name in AGENT_NAMES.items():
        if aid == agent_id:
            short_name = name.lower()
            break
    if not short_name:
        # Try partial match on the ID
        for aid, name in AGENT_NAMES.items():
            if agent_id in aid or aid in agent_id:
                short_name = name.lower()
                break
    if not short_name:
        short_name = agent_id  # Last resort: use as-is
    
    try:
        conn = sqlite3.connect(db_path)
        row = conn.execute("SELECT env FROM agents WHERE id = ?", (short_name,)).fetchone()
        conn.close()
        if row and row[0]:
            return json.loads(row[0])
    except Exception:
        pass
    return {"LETTA_ACP_BACKEND": "remote", "LETTA_AGENT_ID": agent_id}


# ─── ACP Client (JSON-RPC 2.0 over stdio) ─────────────────────────────────────

class ACPError(Exception):
    pass


class ACPAgent:
    """A single ACP agent subprocess with JSON-RPC communication."""

    def __init__(self, agent_id: str, name: str):
        self.agent_id = agent_id
        self.name = name
        self.process: asyncio.subprocess.Process | None = None
        self.session_id: str | None = None
        self._next_id = 1
        self._pending: dict[int, asyncio.Future] = {}
        self._reader_task: asyncio.Task | None = None

    async def start(self):
        """Spawn the letta-acp subprocess and initialize."""
        agent_env = load_agent_env(self.agent_id)

        env = {
            **os.environ,
            **agent_env,
            "NODE_OPTIONS": "--experimental-websocket",
        }

        self.process = await asyncio.create_subprocess_exec(
            DEFAULT_LETTA_ACP, "--yolo",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=sys.stderr,  # Passthrough for debugging
            env=env,
            cwd="/home/rhomancer",
        )

        self._reader_task = asyncio.create_task(self._read_loop())

        # Initialize
        result = await self._call("initialize", {
            "protocolVersion": 1,
            "clientCapabilities": {},
        }, timeout=30)

        # Create session
        result = await self._call("session/new", {
            "mcpServers": [],
            "cwd": "/home/rhomancer",
            "permissionMode": "unrestricted",
        }, timeout=60)

        self.session_id = result.get("sessionId")
        if not self.session_id:
            raise ACPError("No sessionId in session/new response")

    async def _read_loop(self):
        """Continuously read JSON-RPC messages from the agent's stdout."""
        try:
            while True:
                line = await self.process.stdout.readline()
                if not line:
                    break
                line = line.decode().strip()
                if not line:
                    continue
                try:
                    msg = json.loads(line)
                except json.JSONDecodeError:
                    continue

                msg_id = msg.get("id")

                # Check if this is a response to a pending request
                if msg_id is not None and msg_id in self._pending:
                    fut = self._pending.pop(msg_id)
                    if not fut.done():
                        if "error" in msg:
                            fut.set_exception(ACPError(json.dumps(msg["error"])))
                        else:
                            fut.set_result(msg.get("result", {}))
                elif msg_id is not None and "method" in msg:
                    # Request from agent (e.g., session/request_permission)
                    await self._handle_request(msg)
                elif "method" in msg:
                    # Notification (e.g., session/update) — store for the current prompt
                    await self._handle_notification(msg)
        except (asyncio.CancelledError, ConnectionResetError):
            pass

    async def _handle_request(self, msg: dict):
        """Handle a request from the agent (permissions, etc.)."""
        method = msg.get("method", "")
        params = msg.get("params", {})
        msg_id = msg["id"]

        if method == "session/request_permission":
            # Auto-approve: find allow option
            options = params.get("options", [])
            for opt in options:
                if opt.get("kind") in ("allow_always", "allow_once"):
                    self._send({"jsonrpc": "2.0", "id": msg_id,
                               "result": {"outcome": {"outcome": "selected", "optionId": opt["optionId"]}}})
                    return
            # Fallback: allow first option
            if options:
                self._send({"jsonrpc": "2.0", "id": msg_id,
                           "result": {"outcome": {"outcome": "selected", "optionId": options[0]["optionId"]}}})
                return
        # Default: empty response
        self._send({"jsonrpc": "2.0", "id": msg_id, "result": {}})

    async def _handle_notification(self, msg: dict):
        """Store notifications for the active prompt to consume."""
        # Notifications are read in _read_loop and need to reach the prompt() caller.
        # We use a queue that prompt() drains.
        if not hasattr(self, '_notif_queue'):
            self._notif_queue = asyncio.Queue()
        await self._notif_queue.put(msg)

    def _send(self, msg: dict):
        """Send a JSON-RPC message to the agent."""
        data = (json.dumps(msg) + "\n").encode()
        self.process.stdin.write(data)

    async def _call(self, method: str, params: dict, timeout: float = 120) -> dict:
        """Send a JSON-RPC request and wait for the result."""
        req_id = self._next_id
        self._next_id += 1
        fut: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[req_id] = fut

        self._send({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})

        try:
            return await asyncio.wait_for(fut, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(req_id, None)
            raise ACPError(f"Timeout waiting for {method}")

    async def prompt(self, text: str, on_event=None) -> str:
        """
        Send a prompt and collect the response.
        Returns the full response text.
        on_event(event_dict) is called for each session/update.
        """
        if not self.session_id:
            raise ACPError("No session")

        # Ensure notification queue exists
        if not hasattr(self, '_notif_queue'):
            self._notif_queue = asyncio.Queue()

        # Send session/prompt
        req_id = self._next_id
        self._next_id += 1
        prompt_fut: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[req_id] = prompt_fut

        self._send({
            "jsonrpc": "2.0", "id": req_id,
            "method": "session/prompt",
            "params": {
                "sessionId": self.session_id,
                "prompt": [{"type": "text", "text": text}],
            },
        })

        full_text = ""
        thinking_text = ""

        # Read notifications until the prompt result arrives
        while not prompt_fut.done():
            try:
                # Wait for next notification with short timeout
                msg = await asyncio.wait_for(self._notif_queue.get(), timeout=2.0)
            except asyncio.TimeoutError:
                continue

            if msg.get("method") != "session/update":
                continue

            update = msg.get("params", {}).get("update", {})
            kind = update.get("sessionUpdate", "")

            if kind == "agent_message_chunk":
                chunk = update.get("content", {}).get("text", "")
                full_text += chunk
            elif kind == "agent_thought_chunk":
                chunk = update.get("content", {}).get("text", "")
                thinking_text += chunk

            if on_event:
                await on_event({"kind": kind, "thinking": thinking_text, "text": full_text, "raw": update})

        # Get the result
        try:
            result = await asyncio.wait_for(prompt_fut, timeout=5.0)
        except (asyncio.TimeoutError, ACPError):
            pass

        return full_text

    async def stop(self):
        """Terminate the agent process."""
        if self._reader_task:
            self._reader_task.cancel()
        if self.process:
            try:
                self.process.terminate()
                await asyncio.wait_for(self.process.wait(), timeout=5.0)
            except (asyncio.TimeoutError, ProcessLookupError):
                try:
                    self.process.kill()
                except ProcessLookupError:
                    pass


# ─── Consortium ───────────────────────────────────────────────────────────────

class ConsortiumMessage:
    def __init__(self, sender: str, text: str, msg_type: str = "message"):
        self.sender = sender
        self.text = text
        self.type = msg_type
        self.timestamp = time.time()

    def format(self) -> str:
        return f"[{self.sender}]: {self.text}"


class Consortium:
    def __init__(self, topic: str, agents: list[tuple[str, str]],
                 max_messages: int = 5, initiator: str = "Human",
                 interactive: bool = False, prompt_timeout: int = 180):
        self.topic = topic
        self.agents = agents
        self.max_messages = max_messages
        self.initiator = initiator
        self.interactive = interactive
        self.prompt_timeout = prompt_timeout

        self.acp_agents: dict[str, ACPAgent] = {}
        self.queues: dict[str, asyncio.Queue] = {aid: asyncio.Queue() for aid, _ in agents}
        self.quotas: dict[str, int] = {aid: max_messages for aid, _ in agents}
        self.passed: set[str] = set()
        self.active: set[str] = set()

        self.transcript: list[ConsortiumMessage] = []
        self.lock = asyncio.Lock()
        self.ending = False

    def name(self, aid: str) -> str:
        for agent_id, name in self.agents:
            if agent_id == aid:
                return name
        return aid[:12]

    def all_names(self, exclude: str | None = None) -> str:
        return ", ".join(self.name(aid) for aid, _ in self.agents if aid != exclude)

    def ts(self) -> str:
        return datetime.now().strftime("%H:%M:%S")

    def log(self, msg: str):
        print(f"[{self.ts()}] {msg}", flush=True)

    async def setup(self):
        """Spawn ACP subprocesses for all agents."""
        for agent_id, name in self.agents:
            self.active.add(agent_id)
            self.log(f"Starting {name}...")
            try:
                agent = ACPAgent(agent_id, name)
                await asyncio.wait_for(agent.start(), timeout=90)
                self.acp_agents[agent_id] = agent
                self.log(f"  {name} ready (session: {agent.session_id[:12]}...)")
            except Exception as e:
                self.log(f"  {name} failed: {e}")
                self.active.discard(agent_id)

    async def broadcast(self, sender_id: str, text: str, msg_type: str = "message"):
        msg = ConsortiumMessage(self.name(sender_id), text, msg_type)
        self.transcript.append(msg)
        for aid in self.queues:
            if aid != sender_id:
                await self.queues[aid].put(msg)
        display = text if msg_type == "message" else f"({text})"
        sender_name = self.name(sender_id) if sender_id else "System"
        self.log(f"**[{sender_name}]** {display}")

    def first_prompt(self, aid: str) -> str:
        name = self.name(aid)
        return (
            f"You are {name} in a group discussion (consortium) with: {self.all_names(aid)}.\n\n"
            f"Topic: {self.topic}\n\n"
            f"You have {self.max_messages} messages maximum.\n"
            f"When others speak, you'll see '[Name]: message'.\n"
            f"Respond with your contribution, or exactly 'PASS' if you have nothing to add.\n"
            f"Passing is fine — you stay in the conversation.\n\n"
            f"The discussion starts now.\n[{self.initiator}]: {self.topic}"
        )

    def update_prompt(self, aid: str, msgs: list[ConsortiumMessage]) -> str:
        lines = "\n".join(m.format() for m in msgs)
        return (f"{lines}\n\nRespond with your message, or PASS.\n"
                f"You have {self.quotas[aid]}/{self.max_messages} remaining.")

    def reprompt(self, aid: str, draft: str, msgs: list[ConsortiumMessage]) -> str:
        new = "\n".join(m.format() for m in msgs)
        return (f"While you were composing, new messages arrived:\n\n{new}\n\n"
                f'Your draft was: "{draft}"\n\n'
                f"Revise, keep as-is, or PASS. {self.quotas[aid]}/{self.max_messages} remaining.")

    async def agent_loop(self, aid: str):
        """Main loop for one agent."""
        name = self.name(aid)
        agent = self.acp_agents.get(aid)
        if not agent:
            return

        first = True

        while aid in self.active and not self.ending:
            # Collect new messages
            new_msgs = []
            try:
                first_msg = await asyncio.wait_for(self.queues[aid].get(), timeout=1.0)
                new_msgs.append(first_msg)
                while not self.queues[aid].empty():
                    new_msgs.append(self.queues[aid].get_nowait())
            except asyncio.TimeoutError:
                continue

            prompt = self.first_prompt(aid) if first else self.update_prompt(aid, new_msgs)
            first = False

            self.log(f"*{name} is thinking...*")

            # Stream thinking to console
            async def on_event(event):
                kind = event.get("kind", "")
                if kind == "agent_thought_chunk":
                    thinking = event.get("thinking", "")
                    sys.stdout.write(f"\r  {name} (thinking): {thinking[-80:]}")
                    sys.stdout.flush()
                elif kind == "agent_message_chunk":
                    sys.stdout.write("\r" + " " * 100 + "\r")
                    sys.stdout.flush()

            try:
                response = await asyncio.wait_for(
                    agent.prompt(prompt, on_event=on_event),
                    timeout=self.prompt_timeout
                )
                sys.stdout.write("\r" + " " * 100 + "\r")
                sys.stdout.flush()
            except asyncio.TimeoutError:
                self.log(f"  {name} timed out — PASS")
                self.passed.add(aid)
                continue
            except ACPError as e:
                self.log(f"  {name} error: {e}")
                self.passed.add(aid)
                continue

            response = response.strip()

            if response.upper() == "PASS" or not response:
                self.passed.add(aid)
                self.log(f"  {name}: PASS")
                continue

            # Submit with lock — check for context changes
            async with self.lock:
                missed = []
                while not self.queues[aid].empty():
                    missed.append(self.queues[aid].get_nowait())

                if missed:
                    self.log(f"  {name}: context changed, re-prompting...")
                    try:
                        revised = await asyncio.wait_for(
                            agent.prompt(self.reprompt(aid, response, missed)),
                            timeout=self.prompt_timeout
                        )
                        if revised.strip() and revised.strip().upper() != "PASS":
                            response = revised.strip()
                    except (asyncio.TimeoutError, ACPError):
                        pass  # Keep original

                if response.upper() == "PASS" or not response:
                    self.passed.add(aid)
                    continue

                self.quotas[aid] -= 1
                self.passed.discard(aid)
                await self.broadcast(aid, response)

                if self.quotas[aid] <= 0:
                    self.log(f"  {name} is out of messages")
                    self.active.discard(aid)

    async def human_loop(self):
        if not self.interactive:
            return
        loop = asyncio.get_event_loop()
        while not self.ending:
            try:
                line = await loop.run_in_executor(None, sys.stdin.readline)
                if not line:
                    break
                line = line.strip()
                if line == "/end":
                    self.ending = True
                    break
                if line:
                    msg = ConsortiumMessage("Human", line)
                    self.transcript.append(msg)
                    for aid in self.queues:
                        await self.queues[aid].put(msg)
                    self.log(f"**[Human]** {line}")
            except Exception:
                break

    def should_end(self) -> bool:
        if not self.active:
            return True
        return self.active.issubset(self.passed)

    async def run(self):
        await self.setup()

        if not self.active:
            self.log("No agents available. Exiting.")
            return

        self.log(f"\nStarting consortium: {self.topic}")
        self.log(f"Agents: {', '.join(self.name(aid) for aid, _ in self.agents if aid in self.active)}")
        self.log(f"Max messages per agent: {self.max_messages}\n")

        # Broadcast topic
        for aid in list(self.active):
            await self.queues[aid].put(ConsortiumMessage(self.initiator, self.topic))

        human_task = asyncio.create_task(self.human_loop())

        for cycle in range(100):
            if self.ending:
                break

            tasks = [asyncio.create_task(self.agent_loop(aid)) for aid in list(self.active)]
            if tasks:
                await asyncio.gather(*tasks, return_exceptions=True)

            if self.should_end():
                break

            if self.active.issubset(self.passed):
                await asyncio.sleep(0.5)
                if not any(not self.queues[aid].empty() for aid in self.active):
                    break
                self.passed.clear()

        self.ending = True
        human_task.cancel()

        for agent in self.acp_agents.values():
            await agent.stop()

        self.save_transcript()
        msg_count = sum(1 for m in self.transcript if m.type == "message")
        self.log(f"\nConsortium ended. {msg_count} messages.")
        self.log(f"Transcript: {self.transcript_path}")

    def save_transcript(self):
        ts = datetime.now().strftime("%Y%m%d-%H%M%S")
        slug = re.sub(r'[^a-z0-9]+', '-', self.topic.lower())[:50].strip('-')
        d = Path.home() / "consortium-transcripts"
        d.mkdir(exist_ok=True)
        self.transcript_path = d / f"{ts}-{slug}.md"

        lines = [
            f"# Consortium: {self.topic}",
            f"**Participants:** {', '.join(name for _, name in self.agents)}",
            f"**Transport:** ACP (raw JSON-RPC over stdio)",
            f"**Started:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
            f"**Max messages per agent:** {self.max_messages}",
            "", "---", "",
        ]
        for msg in self.transcript:
            if msg.type == "system":
                lines.append(f"*{msg.text}*")
            else:
                lines.append(f"**[{msg.sender}]** {msg.text}")
            lines.append("")
        lines += ["---", f"**Ended:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"]
        self.transcript_path.write_text("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description="ACP-native multi-agent group chat")
    parser.add_argument("--topic", required=True)
    parser.add_argument("--agents", nargs="+", required=True)
    parser.add_argument("--max-messages", type=int, default=5)
    parser.add_argument("--initiator", default="Human")
    parser.add_argument("--interactive", action="store_true")
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()

    resolved = [resolve_agent(a) for a in args.agents]
    if len(resolved) < 2:
        print("Error: need at least 2 agents", file=sys.stderr)
        sys.exit(1)

    initiator = "Human"
    if args.initiator != "Human" and args.initiator.startswith("agent-"):
        initiator = AGENT_NAMES.get(args.initiator, args.initiator[:12])

    c = Consortium(args.topic, resolved, args.max_messages, initiator,
                   args.interactive, args.timeout)
    asyncio.run(c.run())


if __name__ == "__main__":
    main()
