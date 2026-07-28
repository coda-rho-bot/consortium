#!/usr/bin/env python3
"""
Consortium — Multi-agent group chat via Letta CLI.

Agents participate in living conversations: thinking concurrently, speaking
when they have something to say, reacting to each other in real time.

Uses `letta -p --from-agent` which internally uses ACP.

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
import sys
import time
from datetime import datetime
from pathlib import Path

AGENT_NAMES = {
    "agent-b499137a-e1dd-4427-b9df-73e87adfce9e": "Coda",
    "agent-c51de213-2275-4d1d-9ed4-8ccfb7047e52": "Angus",
    "agent-e6f1a549-e06c-4510-b8ea-506f0ebbd211": "Beacon",
    "agent-2ee946fb-e74c-4628-9d9a-705fa567afb3": "FORGE",
    "agent-5b2254e8-9582-4b39-87be-c9776c958c95": "Sinter",
    "agent-8c1f9353-481d-414c-8e35-2da9c16db269": "Linus",
}


def resolve_agent(name_or_id: str) -> tuple[str, str]:
    """Resolve agent name/short-id to (full_id, display_name)."""
    if name_or_id.startswith("agent-"):
        return name_or_id, AGENT_NAMES.get(name_or_id, name_or_id[:12])
    for aid, name in AGENT_NAMES.items():
        if name.lower() == name_or_id.lower() or name_or_id.lower() in aid:
            return aid, name
    return name_or_id, name_or_id[:12]


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
                 interactive: bool = False, timeout: int = 180):
        self.topic = topic
        self.agents = agents  # list of (agent_id, name)
        self.max_messages = max_messages
        self.initiator = initiator
        self.interactive = interactive
        self.timeout = timeout

        self.conversations: dict[str, str | None] = {aid: None for aid, _ in agents}
        self.queues: dict[str, asyncio.Queue] = {aid: asyncio.Queue() for aid, _ in agents}
        self.quotas: dict[str, int] = {aid: max_messages for aid, _ in agents}
        self.passed: set[str] = set()
        self.active: set[str] = {aid for aid, _ in agents}

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

    async def call_agent(self, agent_id: str, text: str) -> str:
        """Send a message to an agent via letta -p --from-agent. Returns response text."""
        name = self.name(agent_id)
        conv_id = self.conversations.get(agent_id)

        cmd = ["letta", "-p", "--from-agent", os.environ.get("LETTA_AGENT_ID", "consortium")]

        if conv_id:
            cmd += ["--conversation", conv_id]
        else:
            cmd += ["--agent", agent_id]

        cmd += [text, "--output-format", "json"]

        try:
            proc = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd="/home/rhomancer",
            )
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=self.timeout
            )

            result = json.loads(stdout.decode())
            if not conv_id:
                self.conversations[agent_id] = result.get("conversation_id")
            return result.get("result", "").strip()

        except asyncio.TimeoutError:
            self.log(f"  {name} timed out")
            return "PASS"
        except Exception as e:
            self.log(f"  {name} error: {e}")
            return "PASS"

    async def broadcast(self, sender_id: str, text: str, msg_type: str = "message"):
        """Add message to transcript and queue for all other agents."""
        msg = ConsortiumMessage(self.name(sender_id) if sender_id else sender_id, text, msg_type)
        self.transcript.append(msg)
        for aid in self.queues:
            if aid != sender_id:
                await self.queues[aid].put(msg)
        display = text if msg_type == "message" else f"({text})"
        self.log(f"**[{self.name(sender_id) if sender_id else 'System'}]** {display}")

    def build_first_prompt(self, agent_id: str) -> str:
        name = self.name(agent_id)
        return (
            f"You are {name} in a group discussion (consortium) with: {self.all_names(agent_id)}.\n\n"
            f"Topic: {self.topic}\n\n"
            f"You have {self.max_messages} messages maximum.\n"
            f"When others speak, you'll see '[Name]: message'.\n"
            f"Respond with your contribution, or exactly 'PASS' if you have nothing to add.\n"
            f"Passing is fine — you stay in the conversation.\n\n"
            f"The discussion starts now.\n"
            f"[{self.initiator}]: {self.topic}"
        )

    def build_update_prompt(self, agent_id: str, messages: list[ConsortiumMessage]) -> str:
        remaining = self.quotas[agent_id]
        lines = "\n".join(m.format() for m in messages)
        return (
            f"{lines}\n\n"
            f"Respond with your message, or PASS.\n"
            f"You have {remaining}/{self.max_messages} remaining."
        )

    def build_reprompt(self, agent_id: str, draft: str,
                       messages: list[ConsortiumMessage]) -> str:
        remaining = self.quotas[agent_id]
        new = "\n".join(m.format() for m in messages)
        return (
            f"While you were composing, new messages arrived:\n\n{new}\n\n"
            f'Your draft was: "{draft}"\n\n'
            f"Revise, keep as-is, or PASS. {remaining}/{self.max_messages} remaining."
        )

    async def agent_loop(self, agent_id: str):
        """Main async loop for one agent."""
        name = self.name(agent_id)
        first = True

        while agent_id in self.active and not self.ending:
            # Collect new messages
            new_msgs = []
            try:
                first_msg = await asyncio.wait_for(self.queues[agent_id].get(), timeout=1.0)
                new_msgs.append(first_msg)
                while not self.queues[agent_id].empty():
                    new_msgs.append(self.queues[agent_id].get_nowait())
            except asyncio.TimeoutError:
                continue

            # Build prompt
            prompt = self.build_first_prompt(agent_id) if first else self.build_update_prompt(agent_id, new_msgs)
            first = False

            self.log(f"*{name} is thinking...*")

            # Call agent
            response = await self.call_agent(agent_id, prompt)

            # Parse response
            if response.upper() == "PASS" or not response:
                self.passed.add(agent_id)
                self.log(f"  {name}: PASS")
                continue

            # Submit with lock
            async with self.lock:
                # Check for messages that arrived during generation
                missed = []
                while not self.queues[agent_id].empty():
                    missed.append(self.queues[agent_id].get_nowait())

                if missed:
                    self.log(f"  {name}: context changed, re-prompting...")
                    revised = await self.call_agent(
                        agent_id, self.build_reprompt(agent_id, response, missed)
                    )
                    if revised.upper() != "PASS" and revised:
                        response = revised

                if response.upper() == "PASS" or not response:
                    self.passed.add(agent_id)
                    continue

                self.quotas[agent_id] -= 1
                self.passed.discard(agent_id)
                await self.broadcast(agent_id, response)

                if self.quotas[agent_id] <= 0:
                    self.log(f"  {name} is out of messages")
                    self.active.discard(agent_id)

    async def human_loop(self):
        """Read human input from stdin."""
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
        if self.active.issubset(self.passed):
            return True
        return False

    async def run(self):
        """Main consortium loop."""
        self.log(f"Starting consortium: {self.topic}")
        self.log(f"Agents: {', '.join(name for _, name in self.agents)}")
        self.log(f"Max messages per agent: {self.max_messages}")
        self.log("")

        # Send topic to all agents
        for aid, _ in self.agents:
            await self.queues[aid].put(ConsortiumMessage(self.initiator, self.topic))

        human_task = asyncio.create_task(self.human_loop())

        max_cycles = 100
        for cycle in range(max_cycles):
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

        self.save_transcript()
        self.log(f"\nConsortium ended. {len(self.transcript)} messages.")
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
    parser = argparse.ArgumentParser(description="Multi-agent group chat (consortium)")
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
